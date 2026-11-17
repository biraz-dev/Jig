// Deterministic races and failure paths for the shipped local settings bridge.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../web/settings.js", import.meta.url), "utf8");
const settle = async () => { for (let index = 0; index < 12; index++) await new Promise(resolve => setImmediate(resolve)); };
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const namedError = (name, message = name) => Object.assign(new Error(message), { name });
const personal = '{"version":1,"tasks":[],"deadlines":[]}';
let checks = 0;

function folder({ access = "granted", writeGate, writeError, closeError, removeGate, removeError, collisions = false } = {}) {
  const files = new Map(), calls = { query: 0, request: 0, close: 0, abort: 0, create: 0, remove: 0 };
  const handle = {
    kind: "directory", name: "Sample backups", files, calls,
    queryPermission: async () => { calls.query++; return access; },
    requestPermission: async () => { calls.request++; access = "granted"; return access; },
    async getFileHandle(name, options) {
      if (!options?.create) {
        if (!files.has(name) && !collisions) throw namedError("NotFoundError");
        return {};
      }
      calls.create++; files.set(name, "");
      return {
        async createWritable() {
          let aborted = false, text;
          return {
            async write(value) {
              const gate = typeof writeGate === "function" ? writeGate() : writeGate;
              if (gate) await gate.promise;
              if (aborted) throw namedError("AbortError");
              const error = typeof writeError === "function" ? writeError() : writeError;
              if (error) throw error;
              text = value;
            },
            async close() {
              if (aborted) throw namedError("AbortError");
              const error = typeof closeError === "function" ? closeError() : closeError;
              if (error) throw error;
              calls.close++; files.set(name, text);
            },
            async abort() { calls.abort++; aborted = true; },
          };
        },
      };
    },
    async removeEntry(name) {
      calls.remove++;
      const gate = typeof removeGate === "function" ? removeGate(name, calls.remove) : removeGate;
      if (gate) await gate.promise;
      const error = typeof removeError === "function" ? removeError(name, calls.remove) : removeError;
      if (error) throw error;
      files.delete(name);
    },
  };
  return handle;
}

function closingFolder(gates, options) {
  const handle = folder(options);
  let next = 0;
  handle.calls.closing = 0;
  handle.getFileHandle = async (name, options) => {
    if (!options?.create) {
      if (!handle.files.has(name)) throw namedError("NotFoundError");
      return {};
    }
    handle.calls.create++; handle.files.set(name, "");
    const gate = gates[next++];
    return {
      async createWritable() {
        let text;
        // A real stream's in-progress close need not stop when abort is called.
        const writer = new WritableStream({
          write(value) { text = value; },
          async close() {
            handle.calls.closing++;
            if (gate) await gate.promise;
            handle.calls.close++; handle.files.set(name, text);
          },
          abort() { handle.calls.abort++; },
        }).getWriter();
        return { write: value => writer.write(value), close: () => writer.close(), abort: () => writer.abort() };
      },
    };
  };
  return handle;
}

function bridge({ supported = false, stored = new Map(), handles = new Map(), restored, idbError, picker, errorMetadataBlocked = false } = {}) {
  if (restored) handles.set("backup", restored);
  const nodes = [], downloads = [], listeners = new Map();
  let uuid = 0;
  class Input {
    constructor(tag) { this.tag = tag; this.id = ""; this.type = ""; this.handlers = new Map(); this.focusCalls = []; }
    addEventListener(name, callback) { this.handlers.set(name, callback); }
    setAttribute() {}
    click() {
      if (this.tag === "a") downloads.push({ name: this.download, blob: urls.get(this.href) });
      if (this.type === "file") this.focus();
    }
    focus(options) { this.focusCalls.push(options); document.activeElement = this; }
    remove() { this.removed = true; if (document.activeElement === this) document.activeElement = document.body; }
  }
  const urls = new Map();
  const document = {
    body: { append(node) { nodes.push(node); node.parentElement = this; } },
    createElement: tag => new Input(tag),
    getElementById: id => nodes.find(node => !node.removed && node.id === id) ?? null,
    addEventListener: (name, callback) => listeners.set(name, callback),
  };
  const canvas = document.createElement("canvas"); canvas.id = "jig_canvas";
  document.body.append(canvas); canvas.focus();
  const window = {
    isSecureContext: true,
    crypto: { randomUUID: () => `00000000-0000-0000-0000-${String(++uuid).padStart(12, "0")}` },
    ...(supported ? { showDirectoryPicker: picker ?? (() => Promise.resolve(folder())) } : {}),
  };
  const indexedDB = {
    open() {
      const request = {};
      queueMicrotask(() => {
        if (idbError) { request.error = idbError; request.onerror(); return; }
        request.result = {
          createObjectStore() {}, close() {},
          transaction() {
            const transaction = {};
            transaction.objectStore = () => Object.fromEntries(["get", "put", "delete"].map(method => [method, (...args) => {
              const result = {};
              queueMicrotask(() => {
                if (method === "get") result.result = handles.get(args[0]);
                if (method === "put") handles.set(args[1], args[0]);
                if (method === "delete") handles.delete(args[0]);
                transaction.oncomplete();
              });
              return result;
            }]));
            return transaction;
          },
        };
        request.onsuccess();
      });
      return request;
    },
  };
  vm.runInNewContext(source, {
    window, document, HTMLInputElement: Input, Blob, indexedDB,
    URL: { createObjectURL(blob) { const key = `blob:${urls.size}`; urls.set(key, blob); return key; }, revokeObjectURL() {} },
    localStorage: {
      getItem: key => {
        if (errorMetadataBlocked && key === "jig.backup.error.v1") throw new Error("Synthetic metadata storage failure");
        return stored.get(key) ?? null;
      },
      setItem: (key, value) => {
        if (errorMetadataBlocked && key === "jig.backup.error.v1") throw new Error("Synthetic metadata storage failure");
        stored.set(key, value);
      },
      removeItem: key => {
        if (errorMetadataBlocked && key === "jig.backup.error.v1") throw new Error("Synthetic metadata storage failure");
        stored.delete(key);
      },
    },
    setTimeout: callback => setImmediate(callback),
  });
  return {
    window, document, canvas, stored, handles, nodes, downloads,
    status: () => JSON.parse(window.jigBackupStatus()),
    textAgent: () => { const input = document.createElement("input"); input.type = "text"; document.body.append(input); return input; },
    choose: file => {
      const input = nodes.filter(node => node.tag === "input").at(-1);
      input.files = file ? [file] : [];
      return input.handlers.get("change")();
    },
    key: overrides => {
      const event = { target: { id: "jig_canvas" }, key: "e", code: "KeyE", ctrlKey: true, altKey: false,
        metaKey: false, shiftKey: false, isComposing: false, prevented: false, stopped: false,
        preventDefault() { this.prevented = true; }, stopImmediatePropagation() { this.stopped = true; }, ...overrides };
      listeners.get("keydown")(event); return event;
    },
  };
}
async function connected(handle = folder(), options = {}) {
  const context = bridge({ supported: true, picker: () => Promise.resolve(handle), ...options });
  await settle(); context.window.jigBackupChooseFolder(); await settle();
  assert.equal(context.status().folder, handle.name);
  return { ...context, handle };
}

{
  const context = bridge();
  assert.equal(context.window.jigSettingsLoad(), null);
  context.window.jigSettingsSave('{"version":1}');
  assert.equal(context.stored.get("jig.settings.v1"), '{"version":1}');
  assert.equal(context.stored.has("jig.personal.v1"), false);
  context.stored.set("jig.settings.v1", "another tab");
  assert.throws(() => context.window.jigSettingsSave("{}"), /Another tab/);
  assert.equal(context.stored.get("jig.settings.v1"), "another tab");
  checks++;
}
{
  const context = bridge({ stored: new Map([["jig.settings.v1", "λ".repeat(17000)]]) });
  assert.throws(() => context.window.jigSettingsLoad(), /exceeds 32 KiB/);
  assert.throws(() => context.window.jigSettingsSave("λ".repeat(17000)), /exceeds 32 KiB/);
  assert.equal(context.stored.get("jig.settings.v1").length, 17000);
  context.window.jigSettingsExport("{}");
  assert.equal(context.downloads[0].name, "jig-settings.json");
  assert.equal(await context.downloads[0].blob.text(), "{}\n");
  checks++;
}
{
  const context = bridge();
  context.window.jigSettingsImport();
  const input = context.document.activeElement;
  assert.equal(input.id, "jig-settings-import");
  await context.choose({ size: 13, text: async () => '{"version":1}' });
  assert.equal(context.document.activeElement, context.canvas, "Selecting a settings file restores canvas keyboard routing");
  assert.equal(context.canvas.focusCalls.at(-1).preventScroll, true);
  assert.equal(input.removed, true);
  assert.equal(context.window.jigSettingsTakeImport(), '{"version":1}');
  context.window.jigSettingsImport(); await context.choose(null);
  assert.equal(context.document.activeElement, context.canvas, "An empty file selection also restores canvas focus");
  checks++;
}
{
  const context = bridge();
  context.window.jigSettingsImport();
  const input = context.document.activeElement;
  input.handlers.get("cancel")();
  assert.equal(input.removed, true);
  assert.equal(context.document.activeElement, context.canvas, "Canceling the native chooser restores canvas keyboard routing");
  assert.equal(context.window.jigSettingsTakeImport(), null);
  context.window.jigSettingsImport();
  const current = context.document.activeElement;
  input.handlers.get("cancel")();
  await input.handlers.get("change")();
  assert.equal(context.document.activeElement, current, "Late events from an old chooser must not steal a newer chooser's focus");
  checks++;
}
{
  const context = bridge();
  context.canvas.focus = () => { throw new Error("Synthetic focus failure"); };
  context.window.jigSettingsImport();
  await context.choose({ size: 13, text: async () => '{"version":1}' });
  assert.equal(context.window.jigSettingsTakeImport(), '{"version":1}', "A focus failure must not reject settings JSON");
  checks++;
}
for (const rejectEarlier of [false, true]) {
  const context = bridge(), previous = deferred();
  context.window.jigSettingsImport();
  const earlier = context.choose({ size: 10, text: () => previous.promise });
  context.window.jigSettingsImport();
  await context.choose({ size: 10, text: async () => "newer" });
  const editing = context.textAgent(); editing.focus();
  const focusCalls = context.canvas.focusCalls.length;
  if (rejectEarlier) previous.reject(new Error("old failure")); else previous.resolve("older");
  await earlier;
  assert.equal(context.document.activeElement, editing, "A stale asynchronous import must not steal focus on completion");
  assert.equal(context.canvas.focusCalls.length, focusCalls);
  assert.equal(context.window.jigSettingsTakeImport(), "newer");
  assert.equal(context.window.jigSettingsTakeImport(), null);
  assert.equal(context.stored.size, 0);
  checks++;
}
{
  const context = bridge(), reading = deferred();
  context.window.jigSettingsImport();
  const pending = context.choose({ size: 10, text: () => reading.promise });
  context.window.jigSettingsCancelImport(); reading.resolve("canceled"); await pending;
  assert.equal(context.window.jigSettingsTakeImport(), null);
  context.window.jigSettingsImport();
  await context.choose({ size: 32769 });
  assert.throws(() => context.window.jigSettingsTakeImport(), /exceeds/);
  assert.equal(context.window.jigSettingsTakeImport(), null);
  context.window.jigSettingsImport();
  await context.choose({ size: 10, text: async () => "λ".repeat(17000) });
  assert.throws(() => context.window.jigSettingsTakeImport(), /exceeds/);
  checks++;
}
{
  const context = bridge();
  const ctrl = { key: "E", ctrl: true, alt: false, shift: false, meta: false };
  context.window.jigSettingsShortcuts(JSON.stringify([ctrl, { ...ctrl, key: "Comma" }]));
  assert.equal(context.key({}).prevented, true);
  assert.equal(context.key({ target: context.textAgent() }).prevented, true);
  assert.equal(context.key({ shiftKey: true }).prevented, false);
  assert.equal(context.key({ target: { id: "outside" } }).prevented, false);
  assert.equal(context.key({ key: ",", code: "Comma" }).prevented, true);
  assert.equal(context.key({ isComposing: true }).prevented, false);
  const altGr = context.key({ ctrlKey: true, altKey: true, getModifierState: () => true, target: context.textAgent() });
  assert.equal(altGr.prevented, false, "AltGr must retain its native text input action");
  assert.equal(altGr.stopped, true, "AltGr must not become a Ctrl-Alt app action in eframe");
  context.window.jigSettingsShortcuts("[]");
  assert.equal(context.key({}).prevented, false);
  context.window.jigSettingsCapture(true);
  assert.equal(context.key({ key: "q", code: "KeyQ" }).prevented, true);
  assert.equal(context.key({ key: "F5", code: "F5", ctrlKey: false }).prevented, true);
  assert.equal(context.key({ ctrlKey: false }).prevented, false);
  context.window.jigSettingsCapture(false);
  assert.equal(context.key({}).prevented, false);
  assert.throws(() => context.window.jigSettingsShortcuts("{}"), /effective Jig shortcuts/);
  checks++;
}
{
  const context = bridge({ stored: new Map([["jig.personal.v1", personal]]) });
  assert.throws(() => context.window.jigBackupWrite(personal, true), /unavailable/);
  assert.match(context.status().error, /unavailable/);
  assert.equal(context.downloads.length, 0);
  context.window.jigBackupWrite(personal, false);
  assert.match(context.downloads[0].name, /^jig-backup-.*\.json$/);
  assert.equal(await context.downloads[0].blob.text(), personal + "\n");
  assert.ok(context.status().last_backup);
  assert.equal(context.status().error, null, "A successful download fallback resolves its failed backup attempt");
  checks++;
}
{
  const context = bridge({ supported: true }); await settle();
  assert.throws(() => context.window.jigBackupWrite(personal, false), /Choose a backup folder/);
  assert.match(context.status().error, /Choose a backup folder/);
  assert.equal(context.downloads.length, 0);
  assert.equal(context.status().busy, false);
  checks++;
}
{
  const context = await connected();
  context.stored.set("jig.personal.v1", personal);
  context.window.jigBackupWrite(personal, true); await settle();
  const first = [...context.handle.files];
  assert.equal(first.length, 1); assert.equal(first[0][1], personal + "\n");
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.handle.files.size, 2);
  assert.equal(context.handle.files.get(first[0][0]), first[0][1]);
  assert.equal(context.handle.calls.request, 0);
  assert.equal(context.stored.get("jig.personal.v1"), personal);
  assert.equal(context.handles.get("backup"), context.handle);
  assert.equal(context.status().busy, false);
  checks++;
}
{
  const context = await connected(folder({ collisions: true }));
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.handle.calls.create, 0);
  assert.equal(context.handle.calls.close, 0);
  assert.match(context.status().message, /unique backup filename/);
  assert.equal(context.status().busy, false);
  checks++;
}
{
  const handle = folder({ access: "prompt" });
  const context = bridge({ supported: true, restored: handle }); await settle();
  assert.equal(context.status().permission, "prompt");
  assert.equal(handle.calls.request, 0, "Restoring a folder must never prompt for access");
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(handle.files.size, 0); assert.equal(handle.calls.request, 0);
  const failure = context.status().error;
  assert.match(failure, /Reconnect/);
  context.window.jigBackupAuthorizeFolder(); await settle();
  assert.equal(context.status().permission, "granted");
  assert.equal(handle.calls.request, 1);
  assert.equal(context.status().error, failure, "Reconnecting must not claim the failed file was saved");
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(handle.files.size, 1);
  assert.equal(context.status().error, null);
  checks++;
}
{
  const old = deferred(), handle = folder();
  const context = bridge({ supported: true, picker: () => old.promise }); await settle();
  context.window.jigBackupChooseFolder();
  context.window.jigBackupDisconnectFolder(); await settle();
  old.resolve(handle); await settle();
  assert.equal(context.status().folder, null); assert.equal(context.handles.has("backup"), false);
  checks++;
}
{
  const context = await connected();
  context.window.jigBackupDisconnectFolder(); await settle();
  const restored = bridge({ supported: true, stored: context.stored, handles: context.handles }); await settle();
  assert.equal(restored.status().folder, null);
  assert.throws(() => restored.window.jigBackupWrite(personal, false), /Choose/);
  checks++;
}
{
  const gate = deferred(), handle = folder({ writeGate: gate });
  const context = await connected(handle);
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.status().busy, true);
  context.window.jigBackupCancel(); gate.resolve(); await settle();
  assert.equal(handle.calls.close, 0); assert.ok(handle.calls.abort > 0);
  assert.equal(handle.files.size, 0); assert.equal(context.status().last_backup, null);
  assert.equal(context.status().busy, false);
  assert.equal(context.status().error, null, "Canceling an in-flight write must not create a failure");
  checks++;
}
{
  const gate = deferred(), handle = folder({ writeGate: gate });
  const context = await connected(handle);
  context.stored.set("jig.personal.v1", personal);
  context.window.jigBackupWrite(personal, true); await settle();
  context.stored.set("jig.personal.v1", "newer document"); gate.resolve(); await settle();
  assert.equal(handle.calls.close, 0); assert.equal(handle.files.size, 0);
  assert.match(context.status().message, /Personal data changed/);
  assert.match(context.status().error, /Personal data changed/);
  assert.equal(context.status().busy, false);
  checks++;
}
{
  const context = await connected(folder({ writeError: new Error("Synthetic full disk") }));
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.handle.files.size, 0); assert.equal(context.handle.calls.close, 0);
  assert.match(context.status().message, /Synthetic full disk/);
  assert.match(context.status().error, /Synthetic full disk/);
  assert.equal(context.status().last_backup, null); assert.equal(context.status().busy, false);
  assert.throws(() => context.window.jigBackupWrite("λ".repeat(1024 * 1024 + 1), false), /exceeds 2048 KiB/);
  assert.match(context.status().error, /exceeds 2048 KiB/);
  checks++;
}
{
  const context = await connected(folder(), { idbError: new Error("Synthetic handle storage failure") });
  assert.match(context.status().message, /connected for this session/);
  assert.equal(context.status().permission, "granted");
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.handle.files.size, 1, "The explicitly chosen folder remains usable for this session");
  checks++;
}
{
  const stored = new Map([["jig.backup.disconnected.v1", "1"]]);
  const context = await connected(folder(), { stored, idbError: new Error("Synthetic handle storage failure") });
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.handle.files.size, 1, "A newly selected session-only folder must clear an earlier disconnect marker");
  checks++;
}
{
  const context = await connected(folder({ writeError: namedError("NotAllowedError", "Permission revoked during write") }));
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.status().permission, "prompt", "A revoked write permission must make Reconnect available");
  assert.equal(context.handle.files.size, 0);
  checks++;
}
{
  let failing = true, gate = null;
  const handle = folder({ writeError: () => failing ? new Error("Synthetic disk full") : null, writeGate: () => gate });
  const previousTime = "2020-01-01T00:00:00.000Z";
  const context = await connected(handle, { stored: new Map([["jig.backup.last.v1", previousTime]]) });
  handle.files.set("jig-backup-previous.json", personal + "\n");
  handle.files.set("user-owned.txt", "Keep this file");
  const previousFiles = [...handle.files];
  context.window.jigBackupWrite(personal, false); await settle();
  const failure = context.status().error;
  assert.match(failure, /Synthetic disk full/);
  assert.equal(context.stored.get("jig.backup.error.v1"), failure);
  assert.equal(context.status().last_backup, previousTime, "A failed attempt must keep the last successful timestamp");
  assert.deepEqual([...handle.files], previousFiles, "Failure must clean its partial file without touching existing files");

  context.window.jigSettingsLoad(); context.window.jigSettingsSave('{"version":1,"theme":"light"}');
  context.window.jigBackupCancel();
  context.window.jigBackupChooseFolder(); await settle();
  assert.equal(context.status().error, failure, "Settings, cancellation and folder messages must not hide a failed backup");
  assert.match(context.status().message, /Backup folder connected/);

  const latest = '{"version":1,"tasks":[{"id":1,"text":"New work","complete":false}],"deadlines":[]}';
  context.stored.set("jig.personal.v1", latest);
  failing = false; gate = deferred();
  context.window.jigBackupWrite(latest, false); await settle();
  assert.equal(context.status().busy, true);
  assert.equal(context.status().error, failure, "Starting a retry must not claim success before its file closes");
  const created = handle.calls.create;
  assert.throws(() => context.window.jigBackupWrite(latest, false), /still running/);
  assert.equal(handle.calls.create, created, "A duplicate retry must not start another write");
  gate.resolve(); await settle();
  assert.equal(context.status().error, null);
  assert.equal(context.stored.has("jig.backup.error.v1"), false);
  assert.equal(context.status().busy, false);
  assert.notEqual(context.status().last_backup, previousTime);
  for (const [name, text] of previousFiles) assert.equal(handle.files.get(name), text);
  const additions = [...handle.files].filter(([name]) => !previousFiles.some(([previous]) => name === previous));
  assert.equal(additions.length, 1);
  assert.equal(additions[0][1], latest + "\n", "A retry must write the caller's current work, not the earlier failed snapshot");
  assert.equal(context.stored.get("jig.personal.v1"), latest);
  checks++;
}
{
  let failing = true;
  const handle = folder({ closeError: () => failing ? new Error("Synthetic close failure") : null });
  const context = await connected(handle);
  handle.files.set("jig-backup-existing.json", personal);
  context.window.jigBackupWrite(personal, false); await settle();
  assert.match(context.status().error, /Synthetic close failure/);
  assert.equal(context.status().last_backup, null, "A rejected close must never count as a successful backup");
  assert.deepEqual([...handle.files], [["jig-backup-existing.json", personal]]);
  failing = false;
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.status().error, null);
  assert.equal(handle.files.size, 2);
  assert.equal(handle.files.get("jig-backup-existing.json"), personal);
  checks++;
}
{
  const cleanup = deferred(), removed = [];
  const handle = folder({
    writeError: new Error("Synthetic write failure before cleanup"),
    removeGate: (_name, attempt) => attempt === 3 ? cleanup : null,
    removeError: (name, attempt) => {
      removed.push(name);
      return attempt < 3 ? namedError("NoModificationAllowedError", "Native file lock is still releasing") : null;
    },
  });
  const previousTime = "2020-01-01T00:00:00.000Z";
  const context = await connected(handle, { stored: new Map([["jig.backup.last.v1", previousTime]]) });
  handle.files.set("jig-backup-existing.json", personal);
  handle.files.set("user-owned.txt", "Keep this file");
  const existing = [...handle.files];
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(handle.calls.remove, 3, "A transient deletion lock must get bounded cleanup retries");
  assert.equal(context.status().busy, true, "The current write stays busy until cleanup finishes");
  assert.match(context.status().error, /Synthetic write failure before cleanup/);
  assert.equal(context.status().last_backup, previousTime);
  assert.equal(handle.calls.create, 1, "Cleanup must never retry writing the document");
  cleanup.resolve(); await settle();
  assert.equal(context.status().busy, false);
  assert.equal(context.status().last_backup, previousTime);
  assert.deepEqual([...handle.files], existing);
  assert.equal(new Set(removed).size, 1, "All removal attempts must target only this attempt's new file");
  assert.ok(removed.every(name => !existing.some(([previous]) => previous === name)));
  assert.equal(context.stored.get("jig.backup.error.v1"), context.status().error);
  checks++;
}
{
  const handle = folder({
    writeError: new Error("Synthetic write failure with missing placeholder"),
    removeError: name => { handle.files.delete(name); return namedError("NotFoundError"); },
  });
  const context = await connected(handle);
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(handle.calls.remove, 1, "An already removed placeholder needs no cleanup retry");
  assert.equal(handle.files.size, 0);
  assert.equal(context.status().busy, false);
  assert.equal(context.status().error, "Backup failed: Synthetic write failure with missing placeholder");
  checks++;
}
{
  const handle = folder({
    writeError: new Error("Synthetic original write failure"),
    removeError: namedError("NoModificationAllowedError", "Synthetic permanent cleanup lock"),
  });
  const previousTime = "2020-01-01T00:00:00.000Z";
  const context = await connected(handle, { stored: new Map([["jig.backup.last.v1", previousTime]]) });
  handle.files.set("jig-backup-existing.json", personal);
  handle.files.set("user-owned.txt", "Keep this file");
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(handle.calls.remove, 4, "Permanent cleanup failure must stop after a small fixed number of attempts");
  assert.equal(handle.calls.create, 1);
  assert.equal(context.status().busy, false);
  assert.equal(context.status().last_backup, previousTime);
  assert.match(context.status().error, /Synthetic original write failure/);
  assert.match(context.status().error, /Could not remove incomplete backup jig-backup-.*Synthetic permanent cleanup lock/);
  assert.match(context.status().error, /Remove that file manually/);
  assert.ok(context.status().error.length <= 2048);
  assert.equal(context.stored.get("jig.backup.error.v1"), context.status().error);
  assert.equal(handle.files.get("jig-backup-existing.json"), personal);
  assert.equal(handle.files.get("user-owned.txt"), "Keep this file");
  assert.equal([...handle.files.values()].filter(value => value === "").length, 1);
  checks++;
}
{
  const latest = '{"version":1,"tasks":[{"id":2,"text":"Newer saved work","complete":false}],"deadlines":[]}';
  const context = await connected(folder(), { stored: new Map([["jig.personal.v1", latest]]) });
  assert.throws(() => context.window.jigBackupWrite(personal, true), /current successfully saved/);
  assert.match(context.status().error, /current successfully saved/);
  assert.equal(context.handle.calls.create, 0);
  assert.equal(context.stored.get("jig.personal.v1"), latest);
  context.window.jigBackupWrite(latest, true); await settle();
  assert.equal(context.status().error, null);
  assert.deepEqual([...context.handle.files.values()], [latest + "\n"]);
  checks++;
}
{
  let failing = true, gate = null;
  const handle = folder({ writeError: () => failing ? new Error("Old document failure") : null, writeGate: () => gate });
  const context = await connected(handle);
  context.window.jigBackupWrite(personal, false); await settle();
  assert.match(context.status().error, /Old document failure/);
  context.window.jigBackupCancel(true);
  assert.equal(context.status().error, null);
  assert.equal(context.stored.has("jig.backup.error.v1"), false);
  assert.equal(context.status().message, "", "A document reset must not retain the previous document's failure text");

  gate = deferred();
  const old = gate;
  context.window.jigBackupWrite(personal, false); await settle();
  context.window.jigBackupCancel(true);
  failing = false; gate = null;
  const latest = '{"version":1,"tasks":[],"deadlines":[],"notes":[]}';
  context.window.jigBackupWrite(latest, false); await settle();
  const successfulTime = context.status().last_backup;
  old.reject(new Error("Late error from old document")); await settle();
  assert.equal(context.status().error, null, "A canceled write's late error must not affect a new document or retry");
  assert.equal(context.stored.has("jig.backup.error.v1"), false);
  assert.equal(context.status().busy, false);
  assert.equal(context.status().last_backup, successfulTime);
  assert.deepEqual([...handle.files.values()], [latest + "\n"]);
  checks++;
}
{
  const context = await connected(folder({ writeError: new Error("Synthetic disconnect failure") }));
  context.window.jigBackupWrite(personal, false); await settle();
  assert.match(context.status().error, /Synthetic disconnect failure/);
  context.window.jigBackupDisconnectFolder(); await settle();
  assert.equal(context.status().error, null);
  assert.equal(context.stored.has("jig.backup.error.v1"), false);
  assert.equal(context.status().folder, null);
  assert.equal(context.status().last_backup, null);
  checks++;
}
for (const resetFailure of [false, true]) {
  const oldClose = deferred(), newClose = deferred();
  const handle = closingFolder([oldClose, newClose]);
  const previousTime = "2020-01-01T00:00:00.000Z";
  const context = await connected(handle, { stored: new Map([["jig.backup.last.v1", previousTime]]) });
  handle.files.set("jig-backup-existing.json", personal);
  handle.files.set("user-owned.txt", "Keep this file");
  const existing = [...handle.files];
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(handle.calls.closing, 1, "The old backup must already be closing before cancellation");
  const oldName = [...handle.files.keys()].find(name => !existing.some(([previous]) => previous === name));
  assert.ok(oldName);
  context.window.jigBackupCancel(resetFailure);

  const latest = '{"version":1,"tasks":[],"deadlines":[],"notes":[{"id":7,"text":"New document","pinned":false}]}';
  context.window.jigBackupWrite(latest, false); await settle();
  assert.equal(handle.calls.closing, 2);
  assert.equal(context.status().busy, true);
  oldClose.resolve(); await settle();
  assert.equal(handle.files.has(oldName), false, "A canceled close must remove only its newly created file");
  assert.equal(context.status().busy, true, "Old cleanup must not unlock or abort the new writer");
  assert.equal(context.status().last_backup, previousTime, "A canceled close must not advance the success timestamp");
  assert.equal(context.status().error, null, "Canceled close completion must not become a new failure");
  assert.equal(context.stored.has("jig.backup.error.v1"), false);
  for (const [name, text] of existing) assert.equal(handle.files.get(name), text);

  newClose.resolve(); await settle();
  assert.equal(context.status().busy, false);
  assert.equal(context.status().error, null);
  assert.notEqual(context.status().last_backup, previousTime);
  const added = [...handle.files].filter(([name]) => !existing.some(([previous]) => previous === name));
  assert.equal(added.length, 1);
  assert.equal(added[0][1], latest + "\n");
  for (const [name, text] of existing) assert.equal(handle.files.get(name), text);
  checks++;
}
for (const permanent of [false, true]) {
  const oldClose = deferred(), newClose = deferred(), cleanup = deferred();
  const handle = closingFolder([oldClose, newClose], {
    removeGate: (_name, attempt) => attempt === 2 ? cleanup : null,
    removeError: (_name, attempt) => permanent || attempt === 1 ? namedError("NoModificationAllowedError", "Old file cleanup lock") : null,
  });
  const context = await connected(handle);
  handle.files.set("user-owned.txt", "Keep this file");
  context.window.jigBackupWrite(personal, false); await settle();
  const oldName = [...handle.files.keys()].find(name => name !== "user-owned.txt");
  context.window.jigBackupCancel(true);
  const latest = '{"version":1,"tasks":[],"deadlines":[],"notes":[]}';
  context.window.jigBackupWrite(latest, false); await settle();
  oldClose.resolve(); await settle();
  assert.equal(handle.calls.remove, 2);
  assert.equal(context.status().busy, true, "Old cleanup retries must not unlock the current writer");
  assert.equal(context.status().error, null);
  newClose.resolve(); await settle();
  const successfulTime = context.status().last_backup;
  assert.ok(successfulTime);
  assert.equal(context.status().busy, false);
  cleanup.resolve(); await settle();
  assert.equal(handle.calls.remove, permanent ? 4 : 2);
  assert.equal(handle.files.has(oldName), permanent);
  assert.equal(context.status().error, null, "Old cleanup failure must not become the new document's error");
  assert.equal(context.stored.has("jig.backup.error.v1"), false);
  assert.equal(context.status().last_backup, successfulTime);
  assert.equal(context.status().busy, false);
  assert.equal(handle.files.get("user-owned.txt"), "Keep this file");
  assert.equal([...handle.files.values()].filter(text => text === latest + "\n").length, 1);
  checks++;
}
{
  const close = deferred(), handle = closingFolder([close]);
  const context = await connected(handle, { stored: new Map([["jig.personal.v1", personal]]) });
  context.window.jigBackupWrite(personal, true); await settle();
  assert.equal(handle.calls.closing, 1);
  const latest = '{"version":1,"tasks":[],"deadlines":[],"notes":[]}';
  context.stored.set("jig.personal.v1", latest);
  close.resolve(); await settle();
  assert.equal(context.status().error, null, "A normal close keeps its valid point-in-time snapshot when no cancellation occurred");
  assert.ok(context.status().last_backup);
  assert.deepEqual([...handle.files.values()], [personal + "\n"]);
  assert.equal(context.stored.get("jig.personal.v1"), latest);
  checks++;
}
for (const thrown of [null, "x".repeat(3000)]) {
  const gate = deferred();
  const context = await connected(folder({ writeGate: gate }));
  context.window.jigBackupWrite(personal, false); await settle();
  gate.reject(thrown); await settle();
  assert.ok(context.status().error.startsWith("Backup failed: "));
  assert.ok(context.status().error.length <= 2048, "Unexpected failures must remain bounded and readable");
  if (thrown === null) assert.equal(context.status().error, "Backup failed: null");
  assert.equal(context.status().message, context.status().error);
  assert.equal(context.stored.get("jig.backup.error.v1"), context.status().error);
  assert.equal(context.status().busy, false);
  assert.equal(context.handle.files.size, 0);
  assert.equal(context.status().last_backup, null);
  checks++;
}
{
  let failing = true;
  const handle = folder({ writeError: () => failing ? new Error("Synthetic persistent failure") : null });
  const context = await connected(handle);
  context.window.jigBackupWrite(personal, false); await settle();
  const failure = context.status().error;
  assert.equal(context.stored.get("jig.backup.error.v1"), failure);
  const restored = bridge({ supported: true, stored: context.stored, handles: context.handles }); await settle();
  assert.equal(restored.status().error, failure, "Refresh must restore the unresolved failure without a failed-document snapshot");
  assert.match(restored.status().message, /Backup folder connected/);
  assert.equal(restored.status().last_backup, null);
  assert.equal(handle.files.size, 0);
  assert.equal(handle.calls.request, 0, "Restoring failure metadata must not ask for permission");
  failing = false;
  restored.window.jigBackupWrite(personal, false); await settle();
  assert.equal(restored.status().error, null);
  assert.equal(restored.stored.has("jig.backup.error.v1"), false);
  assert.deepEqual([...handle.files.values()], [personal + "\n"]);
  const afterSuccess = bridge({ supported: true, stored: restored.stored, handles: restored.handles }); await settle();
  assert.equal(afterSuccess.status().error, null);
  assert.ok(afterSuccess.status().last_backup);
  checks++;
}
{
  for (const invalid of ["", "x".repeat(2049), 12, { message: "Not text" }]) {
    const context = bridge({ stored: new Map([["jig.backup.error.v1", invalid], ["jig.personal.v1", personal]]) });
    assert.equal(context.status().error, null, "Malformed or oversized optional failure metadata must be ignored");
    assert.equal(context.stored.get("jig.personal.v1"), personal);
  }
  assert.equal(bridge().status().error, null, "Older profiles need no new metadata key");
  checks++;
}
{
  let failing = true;
  const handle = folder({ writeError: () => failing ? new Error("Synthetic write failure with blocked metadata") : null });
  const context = await connected(handle, { errorMetadataBlocked: true, stored: new Map([["jig.personal.v1", personal]]) });
  context.window.jigBackupWrite(personal, false); await settle();
  assert.match(context.status().error, /blocked metadata/);
  assert.equal(context.status().busy, false);
  failing = false;
  context.window.jigBackupWrite(personal, false); await settle();
  assert.equal(context.status().error, null, "A metadata failure must not block successful recovery in memory");
  assert.ok(context.status().last_backup);
  assert.deepEqual([...handle.files.values()], [personal + "\n"]);
  assert.equal(context.stored.get("jig.personal.v1"), personal);
  checks++;
}
console.log(`Local settings bridge checks passed: ${checks} storage, import, shortcut, backup and cancellation cases.`);
