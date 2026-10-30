/* Browser files and storage only. No network, analytics, cookies or backend. */
(() => {
  "use strict";
  const key = "jig.personal.v1";
  const maximumBytes = 2 * 1024 * 1024;
  let observed = null;
  let pending = null;
  let importError = null;
  let unsaved = false;
  window.jigSetUnsaved = value => { unsaved = value; };
  window.addEventListener("beforeunload", event => {
    if (unsaved) { event.preventDefault(); event.returnValue = ""; }
  });

  function unchanged() {
    if (localStorage.getItem(key) !== observed) {
      throw new Error("Another tab changed your Jig data. Export your current work, then reload to load the newer saved copy.");
    }
  }
  window.jigLoad = () => { observed = localStorage.getItem(key); return observed; };
  window.jigSave = text => {
    if (new Blob([text]).size > maximumBytes) throw new Error("State exceeds the 2 MiB browser limit. Export your work to a file.");
    unchanged();
    localStorage.setItem(key, text);
    observed = text;
  };
  window.jigClear = () => { unchanged(); localStorage.removeItem(key); observed = null; };
  window.jigExport = text => {
    const url = URL.createObjectURL(new Blob([text + "\n"], { type: "application/json" }));
    const anchor = document.createElement("a");
    anchor.href = url; anchor.download = "jig.json";
    document.body.append(anchor); anchor.click(); anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  window.jigImport = () => document.getElementById("jig-import").click();
  window.jigTakeImport = () => {
    if (importError) { const message = importError; importError = null; throw new Error(message); }
    const text = pending; pending = null; return text;
  };
  window.addEventListener("DOMContentLoaded", () => {
    document.getElementById("jig-import").addEventListener("change", async event => {
      const file = event.target.files[0];
      event.target.value = "";
      if (!file) return;
      if (file.size > maximumBytes) { importError = "Import exceeds 2 MiB. Use the CLI for larger state files."; return; }
      try { pending = await file.text(); } catch { importError = "Could not read the selected file."; }
    });
  });
  window.jigStarted = () => { document.getElementById("loading").hidden = true; window.jigReady = true; };
  window.jigFailed = message => {
    const loading = document.getElementById("loading"); loading.hidden = false;
    document.getElementById("load-message").textContent = "Could not start the dashboard. Try a browser with WebGL and WebAssembly enabled. " + message;
    window.jigBootError = message;
  };
})();
