/* Local file and clipboard adapters. Document contents never leave this device. */
(() => {
  "use strict";
  const limit = 10 * 1024 * 1024;
  let generation = 0;
  let csvGeneration = 0, odsGeneration = 0, pasteGeneration = 0;
  let csv = null, csvError = null, ods = null, odsError = null;
  let files = [], next = null, fileError = null, currentName = "", currentMime = "";
  let reading = false;
  let paste = null, pasteError = null;
  const workspaceKey = "jig.workspace.v1";
  const workspaces = ["sheet", "charts", "tasks", "notes", "workbench"];
  window.jigPreferredWorkspace = () => {
    try {
      const saved = localStorage.getItem(workspaceKey);
      if (workspaces.includes(saved)) return saved;
    } catch { /* A view preference must not prevent the document from opening. */ }
    return "tasks";
  };
  let workspace = window.jigPreferredWorkspace();
  window.jigWorkspaceMode = mode => {
    if (!workspaces.includes(mode)) return;
    workspace = mode;
    try { localStorage.setItem(workspaceKey, mode); }
    catch { /* The active view still works when preference storage is blocked. */ }
  };
  const takeText = () => {
    if (csvError) { const message = csvError; csvError = null; throw new Error(message); }
    const value = csv; csv = null; return value;
  };
  const takeBytes = kind => {
    const error = kind === "ods" ? odsError : fileError;
    if (error) { if (kind === "ods") odsError = null; else fileError = null; throw new Error(error); }
    const value = kind === "ods" ? ods : next;
    if (kind === "ods") ods = null; else next = null;
    return value ?? new Uint8Array();
  };
  const download = (data, name, type) => {
    const url = URL.createObjectURL(new Blob([data], { type }));
    const anchor = document.createElement("a");
    anchor.href = url; anchor.download = name;
    document.body.append(anchor); anchor.click(); anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const resetCsv = () => { csvGeneration++; csv = null; csvError = null; };
  const resetOds = () => { odsGeneration++; ods = null; odsError = null; };
  const resetDocuments = () => {
    generation++; files = []; next = null; fileError = null; reading = false;
  };
  window.jigCsvImport = () => { resetCsv(); document.getElementById("jig-csv-import").click(); };
  window.jigCsvTake = takeText;
  window.jigCsvCancel = resetCsv;
  window.jigCsvExport = (text, name) => download(text, name, "text/csv;charset=utf-8");
  window.jigOdsImport = () => { resetOds(); document.getElementById("jig-ods-import").click(); };
  window.jigOdsTake = () => takeBytes("ods");
  window.jigBinaryExport = (bytes, name, type) => download(bytes, name, type);
  window.jigDocumentName = () => currentName;
  window.jigDocumentMime = () => currentMime;
  window.jigPdfImport = () => {
    resetDocuments();
    document.getElementById("jig-pdf-import").click();
  };
  window.jigPdfTake = () => takeBytes("pdf");
  window.jigPdfRemaining = () => files.length + (reading ? 1 : 0) + (next ? 1 : 0);
  window.jigPdfNext = async () => {
    if (next || reading || !files.length) return;
    const token = generation, file = files.shift();
    reading = true;
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      if (token !== generation) return;
      currentName = file.name; currentMime = file.type;
      next = bytes;
    } catch {
      if (token === generation) fileError = "Could not read the selected document.";
    } finally { if (token === generation) reading = false; }
  };
  window.jigDocumentCancel = () => {
    resetDocuments(); resetCsv(); resetOds(); pasteGeneration++;
    paste = null; pasteError = null;
    window.jigImportCancel?.();
    window.jigOcrCancel?.();
  };
  window.jigClipboardRead = () => {
    const token = ++pasteGeneration;
    paste = null; pasteError = null;
    navigator.clipboard.readText().then(text => {
      if (token === pasteGeneration) paste = text;
    }).catch(() => {
      if (token === pasteGeneration) pasteError = "Clipboard access was denied. Use Ctrl+V to paste cells.";
    });
  };
  window.jigClipboardTake = () => {
    if (pasteError) { const message = pasteError; pasteError = null; throw new Error(message); }
    const value = paste; paste = null; return value;
  };
  document.addEventListener("keydown", event => {
    const target = event.target;
    const app = target?.id === "jig_canvas" || (target instanceof HTMLInputElement
      && target.type === "text" && !target.id && target.parentElement === document.body);
    if (!app) return;
    const control = (event.ctrlKey || event.metaKey) && !event.altKey;
    // egui owns both text-edit undo and Jig's shared Undo. Native historyUndo
    // can refocus the hidden IME input and replay text from a previous editor.
    if (control && event.key.toLowerCase() === "z") event.preventDefault();
    if (!["sheet", "charts", "tasks"].includes(workspace)) return;
    if ((control && ["s", "o", "p"].includes(event.key.toLowerCase()))
      || (workspace === "sheet" && ((control && event.key.toLowerCase() === "g")
        || (event.shiftKey && event.key === "F11")
        || (event.altKey && ["PageUp", "PageDown"].includes(event.key))))) event.preventDefault();
  }, true);
  window.addEventListener("DOMContentLoaded", () => {
    document.getElementById("jig-csv-import").addEventListener("change", async event => {
      resetCsv();
      const token = csvGeneration;
      const file = event.target.files[0]; event.target.value = "";
      if (!file) return;
      if (file.size > 2 * 1024 * 1024) { csvError = "CSV exceeds the 2 MiB import limit."; return; }
      try { const text = await file.text(); if (token === csvGeneration) csv = text; }
      catch { if (token === csvGeneration) csvError = "Could not read the CSV file."; }
    });
    document.getElementById("jig-ods-import").addEventListener("change", async event => {
      resetOds();
      const token = odsGeneration;
      const file = event.target.files[0]; event.target.value = "";
      if (!file) return;
      if (file.size > limit) { odsError = "ODS exceeds the 10 MiB import limit."; return; }
      try { const bytes = new Uint8Array(await file.arrayBuffer()); if (token === odsGeneration) ods = bytes; }
      catch { if (token === odsGeneration) odsError = "Could not read the ODS document."; }
    });
    document.getElementById("jig-pdf-import").addEventListener("change", event => {
      resetDocuments();
      const chosen = Array.from(event.target.files); event.target.value = "";
      if (!chosen.length) return;
      if (chosen.length > 20 || chosen.some(file => file.size > limit)
        || chosen.reduce((sum, file) => sum + file.size, 0) > 50 * 1024 * 1024) {
        fileError = "Choose at most 20 documents, 10 MiB each and 50 MiB total."; return;
      }
      files = chosen; window.jigPdfNext();
    });
  });
})();
