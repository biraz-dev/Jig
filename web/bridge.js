/* Browser files and storage only. No network, analytics, cookies or backend. */
(() => {
  "use strict";
  // Keep the panel shortcut in Jig while allowing egui to receive the event.
  document.addEventListener("keydown", event => {
    const canvas = document.getElementById("jig_canvas");
    const target = event.target;
    const textAgent = target instanceof HTMLInputElement
      && target.type === "text" && !target.id && target.parentElement === document.body;
    if (canvas && (target === canvas || textAgent)
      && event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey
      && event.key.toLowerCase() === "m") {
      event.preventDefault();
    }
  }, true);
  const key = "jig.personal.v1";
  const maximumBytes = 2 * 1024 * 1024;
  const draftsKey = "jig.drafts.v1";
  const maximumDraftBytes = 128 * 1024;
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
  window.jigLoadDrafts = () => {
    let text;
    try { text = sessionStorage.getItem(draftsKey); }
    catch (error) {
      throw new Error("Could not restore unfinished text from this tab. " + String(error));
    }
    if (text !== null && new Blob([text]).size > maximumDraftBytes) {
      throw new Error("Saved unfinished text exceeds the 128 KiB draft limit. It has been preserved in this tab's storage.");
    }
    return text;
  };
  window.jigSaveDrafts = text => {
    if (typeof text !== "string") throw new Error("Expected JSON text for this tab's unfinished edits.");
    if (new Blob([text]).size > maximumDraftBytes) {
      throw new Error("Unfinished text exceeds the 128 KiB draft limit. Copy the text before reloading.");
    }
    try { sessionStorage.setItem(draftsKey, text); }
    catch (error) {
      throw new Error("Could not save unfinished text for refresh. Copy the text before reloading. " + String(error));
    }
  };
  window.jigClearDrafts = () => {
    try { sessionStorage.removeItem(draftsKey); }
    catch (error) {
      throw new Error("Could not clear this tab's saved unfinished text. Refresh may restore the previous draft. " + String(error));
    }
  };
  window.jigSampleRequested = () => new URLSearchParams(location.search).get("sample") === "1";
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
