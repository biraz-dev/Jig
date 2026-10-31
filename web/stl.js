/* Local STL files only. Model bytes never enter storage or a network request. */
(() => {
  "use strict";
  const maximumBytes = 20 * 1024 * 1024;
  let pending = null, pendingName = "", lastName = "", error = "", request = 0;

  function filename(value, fallback) {
    return String(value ?? "").split(/[\\/]/).at(-1)
      .replace(/[\u0000-\u001f\u007f]/g, "").slice(0, 240) || fallback;
  }

  window.jigStlOpen = () => {
    const input = document.getElementById("jig-stl-import");
    if (!input) { error = "The STL file picker is unavailable. Reload Jig and try again."; return; }
    input.click();
  };
  window.jigStlTakeBytes = () => {
    if (!pending) return new Uint8Array();
    const bytes = pending;
    lastName = pendingName;
    pending = null;
    pendingName = "";
    return bytes;
  };
  // Rust takes the bytes first, then reads the matching filename.
  window.jigStlTakeName = () => lastName;
  window.jigStlTakeError = () => { const message = error; error = ""; return message; };
  window.jigStlClear = () => { request++; pending = null; pendingName = ""; lastName = ""; error = ""; };
  window.jigStlExport = (bytes, name) => {
    const copy = new Uint8Array(bytes);
    const url = URL.createObjectURL(new Blob([copy], { type: "model/stl" }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename(name, "jig-scaled.stl");
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  function connect() {
    document.getElementById("jig-stl-import")?.addEventListener("change", async event => {
      const file = event.target.files[0];
      event.target.value = "";
      const token = ++request;
      pending = null;
      pendingName = "";
      error = "";
      if (!file) return;
      if (file.size === 0) { error = "The selected STL file is empty."; return; }
      if (file.size > maximumBytes) { error = "STL files must be 20 MiB or smaller. The current model has been kept."; return; }
      try {
        const buffer = await file.arrayBuffer();
        if (token !== request) return;
        if (buffer.byteLength !== file.size || buffer.byteLength > maximumBytes) {
          error = "Could not read the complete STL file. The current model has been kept.";
          return;
        }
        pending = new Uint8Array(buffer);
        pendingName = filename(file.name, "model.stl");
      } catch {
        if (token === request) error = "Could not read the STL file. The current model has been kept.";
      }
    });
  }
  if (document.readyState === "loading") window.addEventListener("DOMContentLoaded", connect, { once: true });
  else connect();
})();
