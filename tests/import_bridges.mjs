// Deterministic file-read races against the shipped bridges, without a browser.
// Imported by browser_smoke.mjs; also runnable with node tests/import_bridges.mjs.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const sources = await Promise.all(["bridge.js", "document_bridge.js"].map(name =>
  readFile(new URL(`../web/${name}`, import.meta.url), "utf8")));

function bridge(source) {
  const listeners = new Map();
  const ready = [];
  const inputs = new Map();
  const input = id => {
    if (!inputs.has(id)) inputs.set(id, { addEventListener: (name, handler) =>
      listeners.set(`${id}:${name}`, handler), click() {} });
    return inputs.get(id);
  };
  const window = { addEventListener: (name, handler) => {
    if (name === "DOMContentLoaded") ready.push(handler);
    else listeners.set(name, handler);
  } };
  const clipboard = [];
  const globals = {
    window, Blob, URL, Uint8Array, setTimeout,
    document: { addEventListener() {}, getElementById: input },
    localStorage: { getItem: () => null },
    navigator: { clipboard: { readText: () => clipboard.shift() } },
  };
  for (const script of [source].flat()) vm.runInNewContext(script, globals);
  for (const handler of ready) handler();
  return {
    window, clipboard,
    choose: (id, files) => listeners.get(`${id}:change`)({ target: { files, value: "" } }),
  };
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const bytes = text => new TextEncoder().encode(text);
const file = (text, read) => ({
  name: `${text}.pdf`, type: "application/pdf", size: 10,
  text: () => read ?? Promise.resolve(text),
  arrayBuffer: () => read ?? Promise.resolve(bytes(text)),
});
const settle = () => new Promise(resolve => setImmediate(resolve));

for (const [name, source, input, open, take, limit] of [
  ["JSON", sources[0], "jig-import", "jigImport", "jigTakeImport", 2 * 1024 * 1024],
  ["CSV", sources[1], "jig-csv-import", "jigCsvImport", "jigCsvTake", 2 * 1024 * 1024],
  ["ODS", sources[1], "jig-ods-import", "jigOdsImport", "jigOdsTake", 10 * 1024 * 1024],
]) {
  const decoded = value => value instanceof Uint8Array ? new TextDecoder().decode(value) : value;
  const fresh = () => {
    const context = bridge(source);
    return { ...context, choose: chosen => context.choose(input, [chosen]),
      take: () => decoded(context.window[take]()) };
  };
  {
    const context = fresh(), earlier = deferred();
    const reading = context.choose(file("older", earlier.promise));
    await context.choose(file("newer"));
    earlier.resolve(name === "ODS" ? bytes("older") : "older");
    await reading;
    assert.equal(context.take(), "newer", `${name}: an older read replaced the latest selected file`);
    assert.ok(!context.take(), `${name}: a result was delivered twice`);
  }
  {
    const context = fresh();
    await context.choose(file("previous"));
    await context.choose({ size: limit + 1 });
    assert.throws(context.take, /exceeds/, `${name}: an oversized selection must report its error`);
    assert.ok(!context.take(), `${name}: rejected import revived an older pending result`);
  }
  {
    const context = fresh(), earlier = deferred();
    const reading = context.choose(file("older", earlier.promise));
    await context.choose(file("newer"));
    earlier.reject(new Error("synthetic read failure"));
    await reading;
    assert.equal(context.take(), "newer", `${name}: an old failure replaced a newer success`);
  }
  {
    const context = fresh(), earlier = deferred();
    const reading = context.choose(file("cancelled", earlier.promise));
    context.window[open]();
    earlier.resolve(name === "ODS" ? bytes("cancelled") : "cancelled");
    await reading;
    assert.ok(!context.take(), `${name}: opening a new picker failed to cancel the preceding read`);
  }
}

{
  const { window, choose } = bridge(sources), earlier = deferred();
  const reading = choose("jig-import", [file("cancelled", earlier.promise)]);
  window.jigDocumentCancel();
  earlier.resolve("cancelled");
  await reading;
  assert.equal(window.jigTakeImport(), null, "Switching document formats did not cancel the older JSON read");
}
{
  const { window, choose } = bridge(sources), earlier = deferred();
  const reading = choose("jig-ods-import", [file("cancelled", earlier.promise)]);
  window.jigImport();
  earlier.resolve(bytes("cancelled"));
  await reading;
  assert.equal(window.jigOdsTake().length, 0, "Opening JSON did not cancel the older document read");
}
{
  const { window, choose } = bridge(sources[1]), earlier = deferred();
  choose("jig-pdf-import", [file("older", earlier.promise)]);
  choose("jig-pdf-import", [file("newer")]);
  await settle();
  earlier.resolve(bytes("older"));
  await settle();
  assert.equal(new TextDecoder().decode(window.jigPdfTake()), "newer", "A stale PDF read replaced its successor");
  assert.equal(window.jigDocumentName(), "newer.pdf", "PDF metadata must belong to its delivered bytes");
  assert.equal(window.jigPdfRemaining(), 0);
}
{
  const { window, choose } = bridge(sources[1]), earlier = deferred();
  choose("jig-pdf-import", [file("cancelled", earlier.promise)]);
  window.jigDocumentCancel();
  choose("jig-pdf-import", [file("newer")]);
  await settle();
  earlier.reject(new Error("synthetic read failure"));
  await settle();
  assert.equal(new TextDecoder().decode(window.jigPdfTake()), "newer", "A cancelled PDF read blocked its successor");
  assert.equal(window.jigPdfRemaining(), 0);
}
{
  const { window, choose } = bridge(sources[1]);
  choose("jig-pdf-import", [file("previous")]);
  await settle();
  choose("jig-pdf-import", [{ size: 10 * 1024 * 1024 + 1 }]);
  assert.throws(() => window.jigPdfTake(), /Choose at most/);
  assert.equal(window.jigPdfTake().length, 0, "A rejected PDF selection revived an older result");
}
{
  const { window, choose } = bridge(sources[1]), reading = deferred();
  choose("jig-pdf-import", [file("document", reading.promise)]);
  window.jigCsvCancel();
  reading.resolve(bytes("document"));
  await settle();
  assert.equal(new TextDecoder().decode(window.jigPdfTake()), "document", "Cancelling CSV must not strand an unrelated document read");
  assert.equal(window.jigPdfRemaining(), 0);
}
{
  const { window, clipboard } = bridge(sources[1]), earlier = deferred();
  clipboard.push(earlier.promise, Promise.resolve("newer"));
  window.jigClipboardRead();
  window.jigClipboardRead();
  await settle();
  earlier.reject(new Error("synthetic clipboard failure"));
  await settle();
  assert.equal(window.jigClipboardTake(), "newer", "An older clipboard failure replaced a newer paste");
  clipboard.push(Promise.resolve("cancelled"));
  window.jigClipboardRead();
  window.jigDocumentCancel();
  await settle();
  assert.equal(window.jigClipboardTake(), null, "Cancelled clipboard data became a later paste");
}
console.log("Local import bridge checks passed: latest-selection reads, rejection, cancellation, and matching PDF metadata.");
