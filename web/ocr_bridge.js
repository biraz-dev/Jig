// Optional, on-device OCR. Only fixed same-origin engine assets are fetched.
// Financial files, rendered pages and recognized text stay in browser memory.
(() => {
  "use strict";
  const MAX_BYTES = 10 * 1024 * 1024;
  const MAX_PAGES = 100;
  const MAX_PIXELS = 6_000_000;
  const MAX_TEXT = 1024 * 1024;
  const script = document.currentScript?.src || new URL("ocr_bridge.js", document.baseURI).href;
  const assetBase = new URL("vendor/ocr/", script);
  let generation = 0;
  let active = null;
  let result = "";
  let status = { running: false, progress: 0, stage: "idle", file: "", page: 0, pages: 0, error: "" };
  let tesseractReady;
  let pdfReady;

  function asset(path) {
    const url = new URL(path, assetBase);
    if (url.origin !== location.origin) throw new Error("OCR engine assets must be served by Jig itself.");
    return url.href;
  }

  function current(job) { return active === job && job.generation === generation; }
  function check(job) { if (!current(job)) throw new Error("OCR cancelled."); }
  function update(job, stage, progress) {
    if (!current(job)) return;
    status = { ...status, running: true, stage, progress: Math.max(0, Math.min(1, progress)) };
  }

  function loadTesseract() {
    if (!tesseractReady) {
      tesseractReady = new Promise((resolve, reject) => {
        const element = document.createElement("script");
        element.src = asset("tesseract.min.js");
        element.onload = () => window.Tesseract?.createWorker
          ? resolve(window.Tesseract)
          : reject(new Error("The local OCR engine did not initialize."));
        element.onerror = () => reject(new Error("Could not load Jig's local OCR engine assets."));
        document.head.append(element);
      }).catch(error => { tesseractReady = undefined; throw error; });
    }
    return tesseractReady;
  }

  function loadPdf() {
    if (!pdfReady) {
      pdfReady = import(asset("pdf/pdf.mjs")).then(pdf => {
        pdf.GlobalWorkerOptions.workerSrc = asset("pdf/pdf.worker.mjs");
        return pdf;
      }).catch(error => { pdfReady = undefined; throw error; });
    }
    return pdfReady;
  }

  async function release(job) {
    job.bytes = null;
    if (job.rawWorker) {
      job.rawWorker.terminate();
      job.rawWorker = null;
    }
    if (job.renderTask) {
      try { job.renderTask.cancel(); } catch { /* Already finished. */ }
      job.renderTask = null;
    }
    if (job.canvas) {
      job.canvas.width = 0;
      job.canvas.height = 0;
      job.canvas = null;
    }
    if (job.bitmap) {
      job.bitmap.close();
      job.bitmap = null;
    }
    const pending = [];
    if (job.worker) {
      try { pending.push(Promise.resolve(job.worker.terminate())); } catch { /* Terminated. */ }
      job.worker = null;
    }
    if (job.pdfTask) {
      try { pending.push(Promise.resolve(job.pdfTask.destroy())); } catch { /* Destroyed. */ }
      job.pdfTask = null;
    }
    await Promise.allSettled(pending);
  }

  function cancel() {
    generation++;
    const previous = active;
    active = null;
    result = "";
    status = { running: false, progress: 0, stage: "cancelled", file: "", page: 0, pages: 0, error: "" };
    if (previous) void release(previous);
    if (previous?.rejectCancel) previous.rejectCancel(new Error("OCR cancelled."));
  }

  function imageDimensions(bytes) {
    if (bytes.length >= 24 && bytes[0] === 137 && bytes[1] === 80 && bytes[2] === 78 && bytes[3] === 71) {
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      return [view.getUint32(16), view.getUint32(20)];
    }
    if (bytes[0] === 255 && bytes[1] === 216) {
      let offset = 2;
      while (offset + 4 <= bytes.length) {
        if (bytes[offset++] !== 255) continue;
        while (bytes[offset] === 255) offset++;
        const marker = bytes[offset++];
        if (marker === 217 || marker === 218) break;
        if (marker === 0 || marker === 1 || (marker >= 208 && marker <= 215)) continue;
        if (offset + 2 > bytes.length) break;
        const length = bytes[offset] * 256 + bytes[offset + 1];
        if (length < 2 || offset + length > bytes.length) break;
        if ([192, 193, 194, 195, 197, 198, 199, 201, 202, 203, 205, 206, 207].includes(marker)) {
          if (length < 8) break;
          return [bytes[offset + 5] * 256 + bytes[offset + 6], bytes[offset + 3] * 256 + bytes[offset + 4]];
        }
        offset += length;
      }
    }
    throw new Error("Choose a valid PNG or JPEG screenshot.");
  }

  function boundedSize(width, height) {
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
      throw new Error("The document has invalid page dimensions.");
    }
    const scale = Math.min(1, Math.sqrt(MAX_PIXELS / (width * height)));
    return [Math.max(1, Math.floor(width * scale)), Math.max(1, Math.floor(height * scale))];
  }

  async function createOcrWorker(job) {
    update(job, "Loading local OCR engine", 0);
    const engine = await loadTesseract();
    check(job);
    let rejectInitialization;
    const initializationError = new Promise((_, reject) => { rejectInitialization = reject; });
    const options = {
      workerPath: asset("worker.min.js"),
      corePath: asset("core/"),
      langPath: asset("lang/"),
      gzip: true,
      cacheMethod: "none",
      workerBlobURL: false,
      errorHandler(error) { rejectInitialization(new Error(String(error))); },
      logger(message) {
        if (!current(job)) return;
        const page = Math.max(1, status.page);
        const progress = message.status === "recognizing text" && Number.isFinite(message.progress)
          ? (page - 1 + message.progress) / Math.max(1, status.pages)
          : status.progress;
        update(job, message.status === "recognizing text" ? `Reading page ${page} of ${status.pages}` : "Preparing local OCR engine", progress);
      },
    };
    // Tesseract exposes its worker only after initialization. Capture that
    // synchronous constructor so Cancel can also terminate engine startup.
    // Restore the browser constructor before yielding to any other code.
    const NativeWorker = window.Worker;
    let initialization;
    try {
      window.Worker = class extends NativeWorker {
        constructor(url, settings) {
          super(url, settings);
          job.rawWorker = this;
        }
      };
      initialization = engine.createWorker("eng", 1, options);
    } finally {
      window.Worker = NativeWorker;
    }
    const worker = await Promise.race([initialization, initializationError, job.cancelled]);
    if (!current(job)) {
      await worker.terminate();
      throw new Error("OCR cancelled.");
    }
    job.worker = worker;
    await worker.setParameters({ preserve_interword_spaces: "1" });
    check(job);
    return worker;
  }

  async function recognize(job, canvas, source, page) {
    check(job);
    status = { ...status, page };
    update(job, `Reading page ${page} of ${status.pages}`, (page - 1) / status.pages);
    const recognized = await job.worker.recognize(canvas, {}, { text: true });
    check(job);
    const text = String(recognized.data.text || "");
    job.textBytes += new TextEncoder().encode(text).byteLength;
    if (job.textBytes > MAX_TEXT) throw new Error("Recognized text exceeds the 1 MiB import limit. Split this document into smaller files.");
    const confidence = Number(recognized.data.confidence);
    return { text, confidence: Number.isFinite(confidence) ? Math.max(0, Math.min(100, confidence)) : 0, source, page };
  }

  async function image(job) {
    const [width, height] = imageDimensions(job.bytes);
    if (!width || !height || width * height > MAX_PIXELS) {
      throw new Error("Screenshots are limited to 6 megapixels. Crop or resize this image before importing.");
    }
    status = { ...status, pages: 1, page: 1 };
    await createOcrWorker(job);
    check(job);
    const bitmap = await createImageBitmap(new Blob([job.bytes], { type: job.mime }));
    job.bitmap = bitmap;
    check(job);
    const upscale = Math.max(1, Math.min(2, 1800 / bitmap.width));
    const [renderWidth, renderHeight] = boundedSize(bitmap.width * upscale, bitmap.height * upscale);
    const canvas = document.createElement("canvas");
    job.canvas = canvas;
    canvas.width = renderWidth;
    canvas.height = renderHeight;
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) throw new Error("This browser could not prepare the screenshot for OCR.");
    context.fillStyle = "white";
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    bitmap.close();
    job.bitmap = null;
    job.bytes = null;
    return [await recognize(job, canvas, "image", 1)];
  }

  async function pdf(job) {
    update(job, "Loading local PDF renderer", 0);
    const library = await loadPdf();
    check(job);
    job.pdfTask = library.getDocument({
      data: job.bytes,
      isEvalSupported: false,
      enableXfa: false,
      useSystemFonts: true,
      cMapUrl: asset("pdf/cmaps/"),
      cMapPacked: true,
      standardFontDataUrl: asset("pdf/standard_fonts/"),
      wasmUrl: asset("pdf/wasm/"),
      canvasMaxAreaInBytes: MAX_PIXELS * 4,
      stopAtErrors: true,
    });
    job.bytes = null;
    const document = await job.pdfTask.promise;
    check(job);
    if (!document.numPages || document.numPages > MAX_PAGES) {
      throw new Error("PDF OCR is limited to 100 pages per file. Split this document into smaller files.");
    }
    status = { ...status, pages: document.numPages, page: 1 };
    await createOcrWorker(job);
    const pages = [];
    for (let number = 1; number <= document.numPages; number++) {
      check(job);
      const page = await document.getPage(number);
      check(job);
      const original = page.getViewport({ scale: 1 });
      const [width, height] = boundedSize(original.width * 3, original.height * 3);
      const viewport = page.getViewport({ scale: Math.min(width / original.width, height / original.height) });
      const canvas = window.document.createElement("canvas");
      job.canvas = canvas;
      canvas.width = Math.max(1, Math.floor(viewport.width));
      canvas.height = Math.max(1, Math.floor(viewport.height));
      const context = canvas.getContext("2d", { alpha: false });
      if (!context) throw new Error("This browser could not render the scanned PDF page.");
      status = { ...status, page: number };
      update(job, `Preparing page ${number} of ${document.numPages}`, (number - 1) / document.numPages);
      job.renderTask = page.render({ canvasContext: context, viewport, annotationMode: 0, background: "rgb(255,255,255)" });
      await job.renderTask.promise;
      job.renderTask = null;
      pages.push(await recognize(job, canvas, "pdf_ocr", number));
      canvas.width = 0;
      canvas.height = 0;
      job.canvas = null;
      page.cleanup();
    }
    return pages;
  }

  function start(input, name, mime) {
    cancel();
    const id = generation;
    let bytes;
    try {
      const length = input?.byteLength ?? input?.length ?? 0;
      if (!length || length > MAX_BYTES) throw new Error("OCR files must be nonempty and no larger than 10 MiB.");
      bytes = input instanceof Uint8Array ? input.slice() : new Uint8Array(input);
      if (!bytes.byteLength || bytes.byteLength > MAX_BYTES) throw new Error("OCR files must be nonempty and no larger than 10 MiB.");
      const pdfMagic = bytes.length >= 5 && [37, 80, 68, 70, 45].every((value, index) => bytes[index] === value);
      const isPdf = pdfMagic || String(mime).toLowerCase() === "application/pdf" || /\.pdf$/i.test(String(name));
      const type = isPdf ? "application/pdf" : bytes[0] === 137 ? "image/png" : "image/jpeg";
      const job = { generation: id, name: String(name), mime: type, bytes, textBytes: 0, worker: null, rawWorker: null, pdfTask: null, canvas: null, bitmap: null, renderTask: null };
      job.cancelled = new Promise((_, reject) => { job.rejectCancel = reject; });
      job.cancelled.catch(() => {});
      active = job;
      status = { running: true, progress: 0, stage: "Preparing document", file: job.name, page: 0, pages: 0, error: "" };
      void (async () => {
        try {
          const pages = await Promise.race([isPdf ? pdf(job) : image(job), job.cancelled]);
          check(job);
          result = JSON.stringify({ name: job.name, pages });
          status = { ...status, running: false, progress: 1, stage: "Ready for review", error: "" };
        } catch (error) {
          if (current(job)) {
            const message = String(error?.message || error || "Could not read this document.");
            result = JSON.stringify({ name: job.name, pages: [], error: message });
            status = { ...status, running: false, stage: "Could not read document", error: message };
          }
        } finally {
          await release(job);
          if (active === job) active = null;
        }
      })();
    } catch (error) {
      const message = String(error?.message || error);
      result = JSON.stringify({ name: String(name), pages: [], error: message });
      status = { running: false, progress: 0, stage: "Could not read document", file: String(name), page: 0, pages: 0, error: message };
    }
    return id;
  }

  window.jigOcrAvailable = () => typeof Worker === "function" && typeof WebAssembly === "object" && typeof createImageBitmap === "function";
  window.jigOcrStart = start;
  window.jigOcrTake = () => { const value = result; result = ""; return value; };
  window.jigOcrStatus = () => JSON.stringify(status);
  window.jigOcrCancel = cancel;
})();
