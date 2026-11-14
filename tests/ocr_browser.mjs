// Real local OCR using synthetic screenshots and image-only PDFs. No personal files.
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const requests = [], issues = [];
const browser = await chromium.launch({ headless: true, args: ["--disable-dev-shm-usage"] });
const context = await browser.newContext({ viewport: { width: 1000, height: 700 } });
// Exercise the existing Sheet/tool flow using a normal saved workspace preference.
await context.addInitScript(path => {
  if (location.pathname === path) localStorage.setItem("jig.workspace.v1", "sheet");
}, new URL(base).pathname);
context.on("request", request => requests.push({ url: request.url(), method: request.method() }));
await context.route("**/*", async route => {
  const url = new URL(route.request().url());
  if (["data:", "blob:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
  else { issues.push(`Unexpected remote request: ${url.href}`); await route.abort(); }
});
const page = await context.newPage();
page.on("pageerror", error => issues.push(error.message));
await page.route("**/ocr-harness", route => route.fulfill({ contentType: "text/html", body: '<!doctype html><title>Sample local OCR test</title><script src="ocr_bridge.js"></script>' }));

function makePdf(image, pages = 1) {
  const objects = [];
  objects.push(Buffer.from("<< /Type /Catalog /Pages 2 0 R >>"));
  const kids = Array.from({ length: pages }, (_, index) => `${3 + index} 0 R`).join(" ");
  objects.push(Buffer.from(`<< /Type /Pages /Count ${pages} /Kids [${kids}] >>`));
  const imageId = 3 + pages, contentId = imageId + 1;
  for (let index = 0; index < pages; index++) {
    objects.push(Buffer.from(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 180] /Resources << /XObject << /Im0 ${imageId} 0 R >> >> /Contents ${contentId} 0 R >>`));
  }
  objects.push(Buffer.concat([Buffer.from(`<< /Type /XObject /Subtype /Image /Width 1800 /Height 540 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length ${image.length} >>\nstream\n`), image, Buffer.from("\nendstream")]));
  const content = "q 600 0 0 180 0 0 cm /Im0 Do Q";
  objects.push(Buffer.from(`<< /Length ${content.length} >>\nstream\n${content}\nendstream`));
  const parts = [Buffer.from("%PDF-1.4\n%\xff\xff\xff\xff\n", "binary")], offsets = [0];
  let position = parts[0].length;
  objects.forEach((object, index) => {
    offsets.push(position);
    const block = Buffer.concat([Buffer.from(`${index + 1} 0 obj\n`), object, Buffer.from("\nendobj\n")]);
    parts.push(block); position += block.length;
  });
  const xref = `xref\n0 ${offsets.length}\n0000000000 65535 f \n${offsets.slice(1).map(offset => `${String(offset).padStart(10, "0")} 00000 n \n`).join("")}trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${position}\n%%EOF\n`;
  parts.push(Buffer.from(xref));
  return Buffer.concat(parts);
}

async function run(bytes, name, mime) {
  await page.evaluate(({ encoded, name, mime }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    window.jigOcrStart(bytes, name, mime);
  }, { encoded: bytes.toString("base64"), name, mime });
  await page.waitForFunction(() => !JSON.parse(window.jigOcrStatus()).running, undefined, { timeout: 120000 });
  const result = JSON.parse(await page.evaluate(() => window.jigOcrTake()));
  await page.waitForTimeout(100);
  return result;
}

try {
  await page.goto(new URL("ocr-harness", base).href);
  assert.equal(await page.evaluate(() => window.jigOcrAvailable()), true);
  assert.equal(requests.filter(request => request.url.includes("/vendor/ocr/")).length, 0, "OCR assets must not load at startup");
  const fixture = await page.evaluate(() => {
    const canvas = document.createElement("canvas");
    canvas.width = 1800; canvas.height = 540;
    const context = canvas.getContext("2d");
    context.fillStyle = "white"; context.fillRect(0, 0, canvas.width, canvas.height);
    context.fillStyle = "black"; context.font = "bold 38px monospace";
    context.fillText("SAMPLE BANK TRANSACTIONS", 50, 70);
    context.font = "32px monospace";
    ["Date              Description                    Amount", "2026-11-10        Art supply shop                -45.20", "2026-11-11        Studio refund                   12.00", "Opening balance 1000.00", "Closing balance  966.80"].forEach((line, index) => context.fillText(line, 50, 140 + index * 65));
    return { png: canvas.toDataURL("image/png").split(",")[1], jpeg: canvas.toDataURL("image/jpeg", 0.97).split(",")[1] };
  });
  const screenshot = Buffer.from(fixture.png, "base64");
  console.log("OCR: real synthetic PNG recognition");
  const image = await run(screenshot, "Sample transactions.png", "image/png");
  assert.equal(image.error, undefined, JSON.stringify(image));
  assert.equal(image.pages.length, 1);
  assert.equal(image.pages[0].source, "image");
  assert.match(image.pages[0].text, /2026-11-10/);
  assert.match(image.pages[0].text, /-45\.20/);
  assert.match(image.pages[0].text, /12\.00/);
  assert.ok(image.pages[0].confidence >= 60);
  console.log(JSON.stringify({ image: image.pages[0] }));
  console.log("OCR: real synthetic image-only PDF recognition");
  const scannedPdf = makePdf(Buffer.from(fixture.jpeg, "base64"));
  const pdf = await run(scannedPdf, "Sample scanned statement.pdf", "application/pdf");
  assert.equal(pdf.error, undefined, JSON.stringify(pdf));
  assert.equal(pdf.pages.length, 1);
  assert.equal(pdf.pages[0].source, "pdf_ocr");
  assert.match(pdf.pages[0].text, /2026-11-10/);
  assert.match(pdf.pages[0].text, /-45\.20/);
  assert.match(pdf.pages[0].text, /12\.00/);
  console.log(JSON.stringify({ pdf: pdf.pages[0] }));
  console.log("OCR: page/file/image bounds and cancellation");
  const excessive = await run(makePdf(Buffer.from(fixture.jpeg, "base64"), 101), "Sample over-limit.pdf", "application/pdf");
  assert.match(excessive.error, /100 pages/);
  await page.evaluate(() => window.jigOcrStart(new Uint8Array(10 * 1024 * 1024 + 1), "Sample large.png", "image/png"));
  assert.match(JSON.parse(await page.evaluate(() => window.jigOcrTake())).error, /10 MiB/);
  await page.evaluate(() => {
    const bytes = new Uint8Array(24);
    bytes.set([137, 80, 78, 71]);
    const view = new DataView(bytes.buffer);
    view.setUint32(16, 3000); view.setUint32(20, 3000);
    window.jigOcrStart(bytes, "Sample oversized pixels.png", "image/png");
  });
  await page.waitForFunction(() => !JSON.parse(window.jigOcrStatus()).running);
  assert.match(JSON.parse(await page.evaluate(() => window.jigOcrTake())).error, /6 megapixels/);
  const workerEvent = page.waitForEvent("worker", { timeout: 15000 });
  await page.evaluate(encoded => window.jigOcrStart(Uint8Array.from(atob(encoded), character => character.charCodeAt(0)), "Sample cancelled.png", "image/png"), screenshot.toString("base64"));
  const worker = await workerEvent;
  const closed = new Promise(resolve => worker.once("close", resolve));
  await page.evaluate(() => window.jigOcrCancel());
  let closeDeadline;
  try {
    await Promise.race([closed, new Promise((_, reject) => {
      closeDeadline = setTimeout(() => reject(new Error("The cancelled OCR worker did not terminate")), 10000);
    })]);
  } finally { clearTimeout(closeDeadline); }
  await page.waitForTimeout(250);
  assert.equal(JSON.parse(await page.evaluate(() => window.jigOcrStatus())).running, false);
  assert.equal(await page.evaluate(() => window.jigOcrTake()), "");
  assert.deepEqual(await page.evaluate(() => Object.keys(localStorage)), []);
  assert.deepEqual(issues, []);
  assert.ok(requests.every(request => request.method === "GET" && new URL(request.url).origin === origin), "Only same-origin static assets may be requested; documents must never upload");
  console.log("OCR browser passed: real local PNG and image-PDF recognition, date/sign/amount preservation, worker-startup cancellation, bounds, lazy self-hosted assets, no persistent document data, and no external/upload requests.");
} finally {
  await context.close();
  await browser.close();
}
