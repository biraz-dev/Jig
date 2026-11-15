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

function folder({ access = "granted", writeGate, writeError, collisions = false } = {}) {
  const files = new Map(), calls = { query: 0, request: 0, close: 0, abort: 0, create: 0 };
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
              if (writeGate) await writeGate.promise;
              if (aborted) throw namedError("AbortError");
              if (writeError) throw writeError;
              text = value;
            },
            async close() {
              if (aborted) throw namedError("AbortError");
              calls.close++; files.set(name, text);
            },
            async abort() { calls.abort++; aborted = true; },
          };
        },
      };
    },
    async removeEntry(name) { files.delete(name); },
  };
  return handle;
}

function bridge({ supported = false, stored = new Map(), handles = new Map(), restored, idbError, picker } = {}) {
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
      getItem: key => stored.get(key) ?? null,
      setItem: (key, value) => stored.set(key, value),
      removeItem: key => stored.delete(key),
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
  assert.throws(() => context.window.jigBackupWrite(personal, false, false), /Sample data/);
  assert.throws(() => context.window.jigBackupWrite(personal, true, true), /unavailable/);
  assert.equal(context.downloads.length, 0);
  context.window.jigBackupWrite(personal, false, true);
  assert.match(context.downloads[0].name, /^jig-backup-.*\.json$/);
  assert.equal(await context.downloads[0].blob.text(), personal + "\n");
  assert.ok(context.status().last_backup);
  checks++;
}
{
  const context = bridge({ supported: true }); await settle();
  assert.throws(() => context.window.jigBackupWrite(personal, false, true), /Choose a backup folder/);
  assert.equal(context.downloads.length, 0);
  assert.equal(context.status().busy, false);
  checks++;
}
{
  const context = await connected();
  context.stored.set("jig.personal.v1", personal);
  context.window.jigBackupWrite(personal, true, true); await settle();
  const first = [...context.handle.files];
  assert.equal(first.length, 1); assert.equal(first[0][1], personal + "\n");
  context.window.jigBackupWrite(personal, false, true); await settle();
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
  context.window.jigBackupWrite(personal, false, true); await settle();
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
  context.window.jigBackupWrite(personal, false, true); await settle();
  assert.equal(handle.files.size, 0); assert.equal(handle.calls.request, 0);
  context.window.jigBackupAuthorizeFolder(); await settle();
  assert.equal(context.status().permission, "granted");
  assert.equal(handle.calls.request, 1);
  context.window.jigBackupWrite(personal, false, true); await settle();
  assert.equal(handle.files.size, 1);
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
  assert.throws(() => restored.window.jigBackupWrite(personal, false, true), /Choose/);
  checks++;
}
{
  const gate = deferred(), handle = folder({ writeGate: gate });
  const context = await connected(handle);
  context.window.jigBackupWrite(personal, false, true); await settle();
  assert.equal(context.status().busy, true);
  context.window.jigBackupCancel(); gate.resolve(); await settle();
  assert.equal(handle.calls.close, 0); assert.ok(handle.calls.abort > 0);
  assert.equal(handle.files.size, 0); assert.equal(context.status().last_backup, null);
  assert.equal(context.status().busy, false);
  checks++;
}
{
  const gate = deferred(), handle = folder({ writeGate: gate });
  const context = await connected(handle);
  context.stored.set("jig.personal.v1", personal);
  context.window.jigBackupWrite(personal, true, true); await settle();
  context.stored.set("jig.personal.v1", "newer document"); gate.resolve(); await settle();
  assert.equal(handle.calls.close, 0); assert.equal(handle.files.size, 0);
  assert.match(context.status().message, /Personal data changed/);
  assert.equal(context.status().busy, false);
  checks++;
}
{
  const context = await connected(folder({ writeError: new Error("Synthetic full disk") }));
  context.window.jigBackupWrite(personal, false, true); await settle();
  assert.equal(context.handle.files.size, 0); assert.equal(context.handle.calls.close, 0);
  assert.match(context.status().message, /Synthetic full disk/);
  assert.equal(context.status().last_backup, null); assert.equal(context.status().busy, false);
  assert.throws(() => context.window.jigBackupWrite("λ".repeat(1024 * 1024 + 1), false, true), /exceeds 2048 KiB/);
  checks++;
}
{
  const context = await connected(folder(), { idbError: new Error("Synthetic handle storage failure") });
  assert.match(context.status().message, /connected for this session/);
  assert.equal(context.status().permission, "granted");
  context.window.jigBackupWrite(personal, false, true); await settle();
  assert.equal(context.handle.files.size, 1, "The explicitly chosen folder remains usable for this session");
  checks++;
}
{
  const stored = new Map([["jig.backup.disconnected.v1", "1"]]);
  const context = await connected(folder(), { stored, idbError: new Error("Synthetic handle storage failure") });
  context.window.jigBackupWrite(personal, false, true); await settle();
  assert.equal(context.handle.files.size, 1, "A newly selected session-only folder must clear an earlier disconnect marker");
  checks++;
}
{
  const context = await connected(folder({ writeError: namedError("NotAllowedError", "Permission revoked during write") }));
  context.window.jigBackupWrite(personal, false, true); await settle();
  assert.equal(context.status().permission, "prompt", "A revoked write permission must make Reconnect available");
  assert.equal(context.handle.files.size, 0);
  checks++;
}
console.log(`Local settings bridge checks passed: ${checks} storage, import, shortcut, backup and cancellation cases.`);
