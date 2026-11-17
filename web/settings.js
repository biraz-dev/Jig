/* Local application preferences and user-selected backup files. No network. */
(() => {
  "use strict";
  const settingsKey = "jig.settings.v1";
  const settingsLimit = 32 * 1024;
  const backupLimit = 2 * 1024 * 1024;
  const disabledKey = "jig.backup.disconnected.v1";
  const lastBackupKey = "jig.backup.last.v1";
  const backupErrorKey = "jig.backup.error.v1";
  let observedSettings = null;
  let importGeneration = 0;
  let importInput = null;
  let importedSettings = null;
  let importError = null;
  let shortcuts = [];
  let capturingShortcut = false;

  const punctuation = {
    ",": "Comma", ".": "Period", "/": "Slash", "\\": "Backslash", "-": "Minus",
    "=": "Equals", ";": "Semicolon", "'": "Quote", "`": "Backtick", "[": "OpenBracket", "]": "CloseBracket",
  };
  const punctuationCodes = new Set(Object.values(punctuation));
  const codeAliases = { Equal: "Equals", BracketLeft: "OpenBracket", BracketRight: "CloseBracket", Backquote: "Backtick" };
  function shortcutKey(event) {
    if (/^[a-z0-9]$/i.test(event.key)) return event.key.toUpperCase();
    if (/^F(?:[1-9]|1[0-2])$/.test(event.key)) return event.key;
    if (punctuation[event.key]) return punctuation[event.key];
    if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3);
    if (/^(?:Digit|Numpad)[0-9]$/.test(event.code)) return event.code.slice(-1);
    return codeAliases[event.code] ?? (punctuationCodes.has(event.code) ? event.code : null);
  }
  window.jigSettingsShortcuts = text => {
    bounded(text, settingsLimit, "Shortcuts");
    const next = JSON.parse(text);
    if (!Array.isArray(next) || next.length > 12 || next.some(chord =>
      !chord || typeof chord.key !== "string"
      || ["ctrl", "shift", "alt", "meta"].some(name => typeof chord[name] !== "boolean"))) {
      throw new Error("Expected the effective Jig shortcuts.");
    }
    shortcuts = next;
  };
  window.jigSettingsCapture = value => { capturingShortcut = value === true; };
  document.addEventListener("keydown", event => {
    const target = event.target;
    const textAgent = target instanceof HTMLInputElement
      && target.type === "text" && !target.id && target.parentElement === document.body;
    if (!(target?.id === "jig_canvas" || textAgent) || event.isComposing) return;
    if (event.ctrlKey && event.altKey && event.getModifierState?.("AltGraph")) {
      // eframe otherwise interprets AltGr typing as a Ctrl-Alt app shortcut.
      // Keep the native text/default action while withholding only this keydown.
      event.stopImmediatePropagation();
      return;
    }
    const key = shortcutKey(event);
    const bound = shortcuts.some(chord => chord.key === key
      && chord.ctrl === event.ctrlKey && chord.shift === event.shiftKey
      && chord.alt === event.altKey && chord.meta === event.metaKey);
    if (bound || capturingShortcut && (event.ctrlKey || event.altKey || event.metaKey || event.key.length !== 1)) {
      event.preventDefault();
    }
  }, true);

  function bounded(text, limit, label) {
    if (typeof text !== "string") throw new Error(`Expected ${label} JSON text.`);
    if (new Blob([text]).size > limit) throw new Error(`${label} exceeds ${limit / 1024} KiB.`);
  }
  function download(text, name) {
    const url = URL.createObjectURL(new Blob([text + "\n"], { type: "application/json" }));
    const anchor = document.createElement("a");
    anchor.href = url; anchor.download = name;
    document.body.append(anchor); anchor.click(); anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  window.jigSettingsLoad = () => {
    observedSettings = localStorage.getItem(settingsKey);
    if (observedSettings !== null) bounded(observedSettings, settingsLimit, "Settings");
    return observedSettings;
  };
  window.jigSettingsSave = text => {
    bounded(text, settingsLimit, "Settings");
    if (localStorage.getItem(settingsKey) !== observedSettings) {
      throw new Error("Another tab changed Jig settings. Reload before saving settings here.");
    }
    localStorage.setItem(settingsKey, text);
    observedSettings = text;
  };
  window.jigSettingsExport = text => {
    bounded(text, settingsLimit, "Settings");
    download(text, "jig-settings.json");
  };
  function removeImportInput(input, token) {
    input.remove();
    if (importInput === input) importInput = null;
    if (token !== importGeneration) return;
    // eframe receives keyboard events on the canvas, not the body left focused
    // when a native file chooser's input is removed.
    try { document.getElementById("jig_canvas")?.focus({ preventScroll: true }); }
    catch { /* A focus failure must not discard an otherwise usable import. */ }
  }
  window.jigSettingsCancelImport = () => {
    ++importGeneration;
    importedSettings = null; importError = null;
    importInput?.remove(); importInput = null;
  };
  window.jigSettingsImport = () => {
    window.jigSettingsCancelImport();
    const token = importGeneration;
    const input = document.createElement("input");
    input.id = "jig-settings-import";
    input.type = "file"; input.accept = ".json,application/json";
    input.hidden = true; input.setAttribute("aria-label", "Import Jig settings JSON");
    importInput = input;
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      removeImportInput(input, token);
      if (!file || token !== importGeneration) return;
      try {
        if (file.size > settingsLimit) throw new Error("Settings exceeds 32 KiB.");
        const text = await file.text();
        bounded(text, settingsLimit, "Settings");
        if (token === importGeneration) importedSettings = text;
      } catch (error) {
        if (token === importGeneration) importError = String(error.message ?? error);
      }
    });
    input.addEventListener("cancel", () => {
      removeImportInput(input, token);
    });
    document.body.append(input);
    try { input.click(); }
    catch (error) { window.jigSettingsCancelImport(); throw error; }
  };
  window.jigSettingsTakeImport = () => {
    if (importError !== null) {
      const error = importError; importError = null; throw new Error(error);
    }
    const text = importedSettings; importedSettings = null; return text;
  };

  const status = {
    supported: window.isSecureContext === true && typeof window.showDirectoryPicker === "function",
    busy: false, folder: null, permission: "unavailable", message: "", last_backup: null, error: null,
  };
  let directory = null;
  let folderGeneration = 0;
  let writeGeneration = 0;
  let activeWriter = null;
  let phase = "idle";
  let sequence = 0;
  try {
    const saved = localStorage.getItem(lastBackupKey);
    if (saved && saved.length <= 40 && Number.isFinite(Date.parse(saved))) status.last_backup = saved;
  } catch { /* A backup can still be downloaded when browser storage is blocked. */ }
  try {
    const saved = localStorage.getItem(backupErrorKey);
    if (typeof saved === "string" && saved.length > 0 && saved.length <= 2048) status.error = saved;
  } catch { /* Keep recovery usable when browser metadata is unavailable. */ }
  window.jigBackupStatus = () => JSON.stringify(status);

  function handleStore(mode, operation) {
    return new Promise((resolve, reject) => {
      if (typeof indexedDB === "undefined") { reject(new Error("Folder storage is unavailable.")); return; }
      const request = indexedDB.open("jig.browser-handles.v1", 1);
      let blocked = false;
      request.onupgradeneeded = () => request.result.createObjectStore("handles");
      request.onerror = () => reject(request.error ?? new Error("Could not open folder storage."));
      request.onblocked = () => { blocked = true; reject(new Error("Folder storage is busy in another tab.")); };
      request.onsuccess = () => {
        const database = request.result;
        if (blocked) { database.close(); return; }
        let transaction, value;
        try {
          transaction = database.transaction("handles", mode);
          value = operation(transaction.objectStore("handles"));
        }
        catch (error) { database.close(); reject(error); return; }
        transaction.oncomplete = () => { database.close(); resolve(value?.result); };
        transaction.onerror = transaction.onabort = () => {
          database.close(); reject(transaction.error ?? new Error("Could not remember the backup folder."));
        };
      };
    });
  }
  async function permission(handle) {
    return typeof handle.queryPermission === "function"
      ? handle.queryPermission({ mode: "readwrite" }) : "unknown";
  }
  function setFolder(handle, access) {
    directory = handle; status.folder = handle?.name ?? null; status.permission = access;
  }
  function report(error, prefix) {
    status.message = `${prefix}: ${String(error?.message ?? error)}`.slice(0, 2048);
  }
  function backupFailure(error) {
    report(error, "Backup failed");
    setBackupError(status.message);
  }
  function setBackupError(message) {
    status.error = message;
    try {
      if (message === null) localStorage.removeItem(backupErrorKey);
      else localStorage.setItem(backupErrorKey, message);
    } catch { /* A metadata failure must not block file backup or recovery. */ }
  }
  function beginFolder(nextPhase) {
    if (phase === "writing") throw new Error("Wait for the current backup to finish.");
    const token = ++folderGeneration;
    phase = nextPhase; status.busy = true;
    return token;
  }
  function finishFolder(token) {
    if (token === folderGeneration) { phase = "idle"; status.busy = false; }
  }
  async function restoreFolder() {
    const token = beginFolder("restoring");
    try {
      if (localStorage.getItem(disabledKey) === "1") return;
      const handle = await handleStore("readonly", store => store.get("backup"));
      if (!handle || token !== folderGeneration) return;
      const access = await permission(handle);
      if (token !== folderGeneration) return;
      setFolder(handle, access);
      status.message = access === "granted" ? "Backup folder connected." : "Reconnect the backup folder to allow writing.";
    } catch (error) {
      if (token === folderGeneration) report(error, "Could not restore the backup folder");
    } finally { finishFolder(token); }
  }
  window.jigBackupChooseFolder = () => {
    if (!status.supported) throw new Error("This browser does not support choosing a backup folder. Use Download backup.");
    const token = beginFolder("choosing");
    status.message = "Choose a backup folder…";
    // Invoke the picker in the user's button event before any asynchronous work.
    let chosen;
    try { chosen = window.showDirectoryPicker({ id: "jig-backups", mode: "readwrite" }); }
    catch (error) { finishFolder(token); throw error; }
    Promise.resolve(chosen).then(async handle => {
      if (token !== folderGeneration) return;
      const access = await permission(handle);
      if (token !== folderGeneration) return;
      localStorage.removeItem(disabledKey);
      setFolder(handle, access);
      try {
        await handleStore("readwrite", store => store.put(handle, "backup"));
        if (token !== folderGeneration) return;
        status.message = "Backup folder connected. Backups use unique JSON filenames.";
      } catch (error) {
        if (token === folderGeneration) report(error, "Folder connected for this session; could not remember it");
      }
    }).catch(error => {
      if (token === folderGeneration) {
        if (error.name === "AbortError") status.message = "Folder selection canceled.";
        else report(error, "Could not choose the backup folder");
      }
    }).finally(() => finishFolder(token));
  };
  window.jigBackupAuthorizeFolder = () => {
    if (!directory) throw new Error("Choose a backup folder first.");
    const token = beginFolder("authorizing");
    const handle = directory;
    let granted;
    try { granted = handle.requestPermission({ mode: "readwrite" }); }
    catch (error) { finishFolder(token); throw error; }
    Promise.resolve(granted).then(access => {
      if (token !== folderGeneration) return;
      status.permission = access;
      status.message = access === "granted" ? "Backup folder reconnected." : "Writing was not allowed. Use Download backup or choose another folder.";
    }).catch(error => { if (token === folderGeneration) report(error, "Could not reconnect the folder"); })
      .finally(() => finishFolder(token));
  };
  window.jigBackupCancel = (resetFailure = false) => {
    ++writeGeneration;
    if (resetFailure) { setBackupError(null); status.message = ""; }
    if (activeWriter) {
      try { Promise.resolve(activeWriter.abort()).catch(() => {}); } catch { /* Already closing. */ }
    }
    activeWriter = null;
    if (phase === "writing") { phase = "idle"; status.busy = false; status.message = "Backup canceled."; }
  };
  window.jigBackupDisconnectFolder = () => {
    window.jigBackupCancel(true);
    const token = beginFolder("disconnecting");
    // A failed IndexedDB delete must not silently reconnect the folder on reload.
    try { localStorage.setItem(disabledKey, "1"); }
    catch (error) { finishFolder(token); throw error; }
    setFolder(null, "unknown");
    handleStore("readwrite", store => store.delete("backup")).then(() => {
      if (token === folderGeneration) status.message = "Backup folder disconnected.";
    }).catch(error => {
      if (token === folderGeneration) report(error, "Folder disconnected; could not remove its remembered handle");
    }).finally(() => finishFolder(token));
  };
  function backupName() {
    const nonce = typeof window.crypto?.randomUUID === "function" ? window.crypto.randomUUID()
      : `${Date.now().toString(36)}-${++sequence}-${Math.random().toString(36).slice(2)}`;
    return `jig-backup-${new Date().toISOString().replace(/[:.]/g, "-")}-${nonce}.json`;
  }
  async function newBackupFile(handle, current) {
    for (let attempt = 0; attempt < 5; attempt++) {
      const name = backupName();
      try { await handle.getFileHandle(name); }
      catch (error) {
        if (error.name !== "NotFoundError") throw error;
        current();
        return { name, file: await handle.getFileHandle(name, { create: true }) };
      }
    }
    throw new Error("Could not create a unique backup filename. Try again.");
  }
  async function removeIncompleteBackup(handle, name) {
    // Native file locks can outlive abort briefly. Retry only this attempt's file.
    const delays = [25, 75, 150];
    for (let attempt = 0; ; attempt++) {
      try { await handle.removeEntry(name); return; }
      catch (error) {
        if (error?.name === "NotFoundError") return;
        if (attempt === delays.length) throw error;
        await new Promise(resolve => setTimeout(resolve, delays[attempt]));
      }
    }
  }
  function rememberBackup() {
    status.last_backup = new Date().toISOString();
    try { localStorage.setItem(lastBackupKey, status.last_backup); } catch { /* File was saved successfully. */ }
  }
  window.jigBackupWrite = (text, automatic) => {
    try {
      bounded(text, backupLimit, "Backup");
      if (automatic && localStorage.getItem("jig.personal.v1") !== text) {
        throw new Error("Automatic backup requires the current successfully saved personal data.");
      }
      if (!status.supported) {
        if (automatic) throw new Error("Automatic folder backups are unavailable in this browser.");
        download(text, backupName()); rememberBackup(); setBackupError(null); status.message = "Backup downloaded."; return;
      }
      if (phase !== "idle") throw new Error("A backup folder operation is still running.");
      if (!directory || localStorage.getItem(disabledKey) === "1") throw new Error("Choose a backup folder first.");
    } catch (error) {
      backupFailure(error);
      throw error;
    }
    const token = ++writeGeneration;
    const handle = directory;
    const current = () => {
      if (token !== writeGeneration) throw new Error("Backup canceled.");
      if (automatic && localStorage.getItem("jig.personal.v1") !== text) throw new Error("Personal data changed before the automatic backup could finish.");
    };
    phase = "writing"; status.busy = true; status.message = "Saving backup…";
    (async () => {
      let stream, name, failure, committed = false;
      try {
        const access = await permission(handle);
        current(); status.permission = access;
        if (access !== "granted") throw new Error("Reconnect the backup folder to allow writing.");
        const target = await newBackupFile(handle, current);
        name = target.name; current();
        stream = await target.file.createWritable();
        current(); activeWriter = stream;
        await stream.write(text + "\n");
        current();
        await stream.close();
        // Closing can finish after cancellation; remove this attempt's new file.
        if (token !== writeGeneration) throw new Error("Backup canceled.");
        committed = true;
        if (token === writeGeneration) { rememberBackup(); setBackupError(null); status.message = `Backup saved to ${handle.name}.`; }
      } catch (error) {
        if (token === writeGeneration) {
          if (error?.name === "NotAllowedError" || error?.name === "SecurityError") status.permission = "prompt";
          backupFailure(error);
          failure = status.error;
        }
      } finally {
        if (!committed) {
          if (stream) { try { await stream.abort(); } catch { /* Already aborted or closed. */ } }
          if (name) {
            try { await removeIncompleteBackup(handle, name); }
            catch (error) {
              if (token === writeGeneration) {
                status.message = `${failure.slice(0, 1024)} Could not remove incomplete backup ${name}: ${String(error?.message ?? error).slice(0, 512)}. Remove that file manually.`;
                setBackupError(status.message);
              }
            }
          }
        }
        if (token === writeGeneration) { activeWriter = null; phase = "idle"; status.busy = false; }
      }
    })();
  };
  if (status.supported) { status.permission = "unknown"; void restoreFolder(); }
})();
