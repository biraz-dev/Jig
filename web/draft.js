/* Draft uses browser text editing; the portable document and saving belong to Rust. */
(() => {
  "use strict";
  const textLimit = 512 * 1024, blockLimit = 4096, runLimit = 16384;
  const recoveryLimit = 4 * 1024 * 1024, pasteLimit = 4 * 1024 * 1024;
  const recoveryKey = "jig.draft.recovery.v1";
  const encoder = new TextEncoder();
  const alignments = ["left", "center", "right", "justify"];
  const listKinds = ["none", "bullet", "number"];
  const blockTags = new Set(["P", "DIV", "H1", "H2", "H3", "H4", "H5", "H6", "PRE", "BLOCKQUOTE", "ADDRESS", "SECTION", "ARTICLE"]);
  const structuralTags = new Set([...blockTags, "LI", "UL", "OL", "TABLE", "TR", "THEAD", "TBODY", "TFOOT"]);
  const discardTags = new Set(["SCRIPT", "STYLE", "META", "LINK", "IMG", "IFRAME", "OBJECT", "EMBED", "SVG", "MATH", "VIDEO", "AUDIO", "SOURCE", "CANVAS", "INPUT", "TEXTAREA", "SELECT", "BUTTON", "TEMPLATE", "NOSCRIPT"]);
  let panel, editor, toolbar, message, recoveryBanner, linkRow, linkInput, sizeInput;
  let wake = () => {}, events = [], visible = false, focusRequested = false;
  let current = {}, durable = {}, currentJSON = "{}", durableJSON = "{}";
  let firstSync = true, applying = false, composing = false, invalidLocal = false;
  let historyPending = false, historyFocus = false, savedRange = null, pendingSize = null;
  let lastTyping = 0, lastTypingKind = "", plainPaste = false;
  let recovered = [], storageProblem = "", recoveryBlocked = false, lastLayout = "", lastPalette = "";

  function bytes(value) { return encoder.encode(value).length; }
  function safeLink(value) {
    if (typeof value !== "string" || bytes(value) > 8192 || /[\s\u0000-\u001f\u007f-\u009f]/u.test(value)) return false;
    const match = /^(https?):\/\/([^/?#]+)|^mailto:(.+)$/i.exec(value);
    return !!match;
  }
  function normalize(value) {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Expected Draft text.");
    const blocks = value.blocks ?? [];
    if (!Array.isArray(blocks) || blocks.length > blockLimit) throw new Error("Draft supports at most 4096 paragraphs or list items.");
    let total = 0, count = 0;
    const normalized = blocks.map(block => {
      if (!block || typeof block !== "object" || Array.isArray(block)) throw new Error("Invalid Draft paragraph.");
      const alignment = block.alignment ?? "left", list = block.list ?? "none";
      if (!alignments.includes(alignment) || !listKinds.includes(list)) throw new Error("Invalid Draft paragraph formatting.");
      if (!Array.isArray(block.runs ?? [])) throw new Error("Invalid Draft text runs.");
      const runs = [];
      for (const run of block.runs ?? []) {
        if (++count > runLimit) throw new Error("Draft supports at most 16384 text runs.");
        if (!run || typeof run !== "object" || typeof run.text !== "string") throw new Error("Invalid Draft text.");
        total += bytes(run.text);
        if (total > textLimit) throw new Error("Draft text must be at most 512 KiB. Copy your text before leaving.");
        if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(run.text)) throw new Error("Draft text contains unsupported control characters.");
        const size = run.size ?? 16;
        if (!Number.isInteger(size) || size < 8 || size > 72) throw new Error("Draft text size must be between 8 and 72.");
        for (const key of ["bold", "italic", "underline"]) if (run[key] !== undefined && typeof run[key] !== "boolean") throw new Error("Invalid Draft text formatting.");
        if (run.link != null && !safeLink(run.link)) throw new Error("Use an http://, https:// or mailto: link without spaces.");
        if (!run.text) continue;
        const next = { text: run.text };
        if (size !== 16) next.size = size;
        for (const key of ["bold", "italic", "underline"]) if (run[key]) next[key] = true;
        if (run.link != null) next.link = run.link;
        const previous = runs.at(-1);
        if (previous && sameStyle(previous, next)) previous.text += next.text;
        else runs.push(next);
      }
      const result = {};
      if (runs.length) result.runs = runs;
      if (alignment !== "left") result.alignment = alignment;
      if (list !== "none") result.list = list;
      return result;
    });
    return normalized.length ? { blocks: normalized } : {};
  }
  function sameStyle(a, b) {
    return ["size", "bold", "italic", "underline", "link"].every(key => a[key] === b[key]);
  }
  function plainText(draft) {
    let number = 0;
    return (draft.blocks ?? []).map(block => {
      const list = block.list ?? "none";
      const prefix = list === "number" ? `${++number}. ` : list === "bullet" ? "• " : "";
      if (list !== "number") number = 0;
      return prefix + (block.runs ?? []).map(run => run.text).join("");
    }).join("\n");
  }
  function element(tag, text, className) {
    const node = document.createElement(tag);
    if (text !== undefined) node.textContent = text;
    if (className) node.className = className;
    return node;
  }
  function fragment(draft) {
    const output = document.createDocumentFragment();
    let listNode = null, previousList = "none";
    for (const block of draft.blocks ?? []) {
      const list = block.list ?? "none";
      if (list !== "none" && (list !== previousList || !listNode)) {
        listNode = element(list === "bullet" ? "ul" : "ol"); output.append(listNode);
      }
      const paragraph = element(list === "none" ? "p" : "li");
      // Clipboard HTML must carry the spacing that the editor's CSS supplies.
      paragraph.style.whiteSpace = "pre-wrap"; paragraph.style.fontSize = "16px";
      if (block.alignment && block.alignment !== "left") paragraph.style.textAlign = block.alignment;
      for (const run of block.runs ?? []) {
        let node = document.createTextNode(run.text);
        for (const [key, tag] of [["underline", "u"], ["italic", "i"], ["bold", "b"]]) {
          if (run[key]) { const wrapper = element(tag); wrapper.append(node); node = wrapper; }
        }
        if (run.size && run.size !== 16) { const wrapper = element("span"); wrapper.style.fontSize = `${run.size}px`; wrapper.append(node); node = wrapper; }
        if (run.link) { const wrapper = element("a"); wrapper.href = run.link; wrapper.rel = "noopener noreferrer"; wrapper.target = "_blank"; wrapper.append(node); node = wrapper; }
        paragraph.append(node);
      }
      if (!paragraph.childNodes.length) paragraph.append(element("br"));
      if (list === "none") { output.append(paragraph); listNode = null; }
      else listNode.append(paragraph);
      previousList = list;
    }
    return output;
  }
  function html(draft) { const holder = element("div"); holder.append(fragment(draft)); return holder.innerHTML; }
  function fontSize(value, legacy = false) {
    if (legacy && /^[1-7]$/.test(value)) return [0, 10, 13, 16, 18, 24, 32, 48][Number(value)];
    const named = { "xx-small": 8, "x-small": 10, small: 13, medium: 16, large: 18, "x-large": 24, "xx-large": 32, "xxx-large": 48 };
    if (named[value]) return named[value];
    const match = /^([\d.]+)(px|pt)$/i.exec(value);
    return match ? Math.max(8, Math.min(72, Math.round(Number(match[1]) * (match[2].toLowerCase() === "pt" ? 4 / 3 : 1)))) : null;
  }
  function readDOM(container) {
    const blocks = []; let block = null, visited = 0, total = 0, count = 0;
    const defaults = { size: 16, bold: false, italic: false, underline: false, link: null, alignment: "left", list: "none", pre: false };
    function flush(force, context) {
      if (block) { blocks.push(block); block = null; }
      else if (force) blocks.push({ runs: [], alignment: context.alignment, list: context.list });
      if (blocks.length > blockLimit) throw new Error("Draft supports at most 4096 paragraphs or list items.");
    }
    function append(text, context) {
      if (!text) return;
      total += bytes(text);
      if (total > textLimit) throw new Error("Draft text must be at most 512 KiB. Copy your text before leaving.");
      block ??= { runs: [], alignment: context.alignment, list: context.list };
      const next = { text, size: context.size, bold: context.bold, italic: context.italic, underline: context.underline, link: context.link };
      const previous = block.runs.at(-1);
      if (previous && sameStyle(previous, next)) previous.text += text;
      else { if (++count > runLimit) throw new Error("Draft supports at most 16384 text runs."); block.runs.push(next); }
    }
    function walk(node, inherited, inline = false) {
      if (++visited > 65536) throw new Error("The pasted text has too much formatting. Paste a smaller selection as plain text.");
      if (node.nodeType === Node.TEXT_NODE) {
        if (!inherited.pre && /^\s+$/.test(node.nodeValue) && [...(node.parentNode?.children ?? [])].some(child => structuralTags.has(child.tagName))) return;
        append(node.nodeValue.replace(/\r\n?/g, "\n"), inherited); return;
      }
      if (node.nodeType !== Node.ELEMENT_NODE && node.nodeType !== Node.DOCUMENT_FRAGMENT_NODE) return;
      const tag = node.tagName;
      if (discardTags.has(tag)) return;
      const context = { ...inherited };
      const style = node.style;
      if (tag === "B" || tag === "STRONG") context.bold = true;
      if (tag === "I" || tag === "EM") context.italic = true;
      if (tag === "U") context.underline = true;
      if (style?.fontWeight) context.bold = style.fontWeight === "bold" || style.fontWeight === "bolder" || Number(style.fontWeight) >= 600;
      if (style?.fontStyle) context.italic = /italic|oblique/.test(style.fontStyle);
      if (style?.textDecorationLine || style?.textDecoration) context.underline = (style.textDecorationLine || style.textDecoration).includes("underline");
      const size = fontSize(style?.fontSize ?? "") ?? (tag === "FONT" ? fontSize(node.getAttribute("size") ?? "", true) : null);
      if (size !== null) context.size = size;
      const alignment = style?.textAlign || node.getAttribute?.("align");
      if (alignments.includes(alignment)) context.alignment = alignment;
      if (tag === "A") { const href = node.getAttribute("href"); context.link = safeLink(href) ? href : null; }
      if (tag === "PRE") context.pre = true;
      if (tag === "BR") {
        const parent = node.parentNode;
        if (parent?.childNodes.length === 1 || !node.nextSibling && node.previousSibling?.nodeName === "BR") return;
        append("\n", context); return;
      }
      if (tag === "UL" || tag === "OL") {
        flush(false, context); context.list = tag === "UL" ? "bullet" : "number";
        for (const child of node.childNodes) walk(child, context);
        flush(false, context); return;
      }
      if (tag === "TR") {
        flush(false, context);
        const cells = [...node.children].filter(child => child.tagName === "TD" || child.tagName === "TH");
        cells.forEach((cell, index) => { if (index) append("\t", context); for (const child of cell.childNodes) walk(child, context, true); });
        flush(true, context); return;
      }
      const paragraph = !inline && (blockTags.has(tag) || tag === "LI");
      const before = blocks.length;
      if (paragraph) flush(false, context);
      const start = blocks.length;
      for (const child of node.childNodes) walk(child, context, inline);
      if (paragraph) flush(blocks.length === start && !block, context);
      // An empty table row is still a useful blank line when gathering text.
      if (tag === "TABLE" && before === blocks.length && !block) flush(true, context);
    }
    for (const child of container.childNodes) walk(child, defaults);
    flush(false, defaults);
    return normalize({ blocks });
  }
  function status(text, error = false) {
    if (!message) return;
    message.textContent = text; message.dataset.error = String(error); message.hidden = !text;
  }
  function resetGroup() { lastTyping = 0; lastTypingKind = ""; }
  function enqueue(event) {
    const last = events.at(-1);
    if (event.kind === "change" && event.coalesce && last?.kind === "change") last.draft = event.draft;
    else events.push(event);
    window.jigSetUnsaved?.(true);
    try { wake(); } catch { /* The recovery journal remains available until the app acknowledges it. */ }
  }
  function journal() {
    // An unreadable journal may be the only copy of earlier text.
    if (recoveryBlocked) return;
    try {
      if (currentJSON === durableJSON && !recovered.length && !invalidLocal) sessionStorage.removeItem(recoveryKey);
      else {
        const text = JSON.stringify({ version: 1, base: durable, draft: current, recovered });
        if (bytes(text) > recoveryLimit) throw new Error("Draft recovery exceeds 4 MiB. Copy your text before reloading.");
        sessionStorage.setItem(recoveryKey, text);
      }
      storageProblem = "";
    } catch (error) {
      storageProblem = `Could not keep a refresh recovery copy. Copy your text before reloading. ${error.message ?? error}`;
      status(storageProblem, true);
    }
  }
  function publish(inputType = "", forceGroup = false) {
    if (applying || historyPending) return;
    try {
      const next = readDOM(editor), json = JSON.stringify(next);
      invalidLocal = false;
      if (json === currentJSON) return;
      const kind = /^insert(?:Text|CompositionText)$/.test(inputType) ? "insert" : /^delete(?:Content|Word)(?:Backward|Forward)$/.test(inputType) ? "delete" : "";
      const now = performance.now();
      const coalesce = !forceGroup && !!kind && kind === lastTypingKind && now - lastTyping < 750;
      current = next; currentJSON = json;
      if (kind && !forceGroup) { lastTyping = now; lastTypingKind = kind; } else resetGroup();
      journal(); enqueue({ kind: "change", draft: next, coalesce });
      if (!storageProblem) status("");
      updateToolbar();
    } catch (error) {
      if (composing) {
        invalidLocal = true; window.jigSetUnsaved?.(true);
      } else {
        // Reject only the latest invalid edit, keeping the last accepted text.
        const bookmark = selectionBookmark(); applying = true;
        editor.replaceChildren(fragment(current)); savedRange = null;
        applying = false; invalidLocal = false; restoreBookmark(bookmark);
      }
      resetGroup(); status(error.message ?? String(error), true);
    }
  }
  function captureRange() {
    const selection = window.getSelection();
    if (selection?.rangeCount && editor.contains(selection.anchorNode) && editor.contains(selection.focusNode)) savedRange = selection.getRangeAt(0).cloneRange();
  }
  function restoreRange() {
    editor.focus({ preventScroll: true });
    const selection = window.getSelection();
    if (savedRange && editor.contains(savedRange.startContainer) && editor.contains(savedRange.endContainer)) {
      selection.removeAllRanges(); selection.addRange(savedRange);
    } else {
      const range = document.createRange(); range.selectNodeContents(editor); range.collapse(false);
      selection.removeAllRanges(); selection.addRange(range); savedRange = range;
    }
  }
  function selectionBookmark() {
    const selection = window.getSelection();
    if (!selection?.rangeCount || !editor.contains(selection.anchorNode) || !editor.contains(selection.focusNode)) return null;
    const offset = (node, index) => { const range = document.createRange(); range.selectNodeContents(editor); range.setEnd(node, index); return range.toString().length; };
    return { anchor: selection.anchorNode, anchorOffset: selection.anchorOffset, focus: selection.focusNode, focusOffset: selection.focusOffset,
      anchorText: offset(selection.anchorNode, selection.anchorOffset), focusText: offset(selection.focusNode, selection.focusOffset) };
  }
  function restoreBookmark(bookmark, reuseNodes = false) {
    if (!bookmark) return;
    function point(node, index, position) {
      if (reuseNodes && editor.contains(node)) return [node, Math.min(index, node.nodeType === Node.TEXT_NODE ? node.length : node.childNodes.length)];
      const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT); let previous = null;
      while ((node = walker.nextNode())) { previous = node; if (position <= node.length) return [node, position]; position -= node.length; }
      return previous ? [previous, previous.length] : [editor, editor.childNodes.length];
    }
    const a = point(bookmark.anchor, bookmark.anchorOffset, bookmark.anchorText), f = point(bookmark.focus, bookmark.focusOffset, bookmark.focusText);
    window.getSelection()?.setBaseAndExtent(a[0], a[1], f[0], f[1]); captureRange();
  }
  function replaceFontSizes() {
    const fonts = [...editor.querySelectorAll('font[size="7"]')];
    if (!fonts.length || pendingSize === null) return;
    const bookmark = selectionBookmark();
    for (const font of fonts) { const span = element("span"); span.style.fontSize = `${pendingSize}px`; span.append(...font.childNodes); font.replaceWith(span); }
    restoreBookmark(bookmark, true);
  }
  function command(name, value = null) {
    if (historyPending) return;
    resetGroup(); restoreRange(); applying = true;
    try { document.execCommand("styleWithCSS", false, true); document.execCommand(name, false, value); }
    finally { applying = false; }
    captureRange(); publish("", true); updateToolbar();
  }
  function resizeText() {
    const size = Number(sizeInput.value);
    if (!Number.isInteger(size) || size < 8 || size > 72 || historyPending) return;
    resetGroup(); restoreRange(); pendingSize = size; applying = true;
    try { document.execCommand("styleWithCSS", false, false); document.execCommand("fontSize", false, "7"); replaceFontSizes(); }
    finally { document.execCommand("styleWithCSS", false, true); applying = false; }
    captureRange(); publish("", true); updateToolbar();
  }
  function updateToolbar() {
    if (!editor || !visible || composing) return;
    const names = { Bold: "bold", Italic: "italic", Underline: "underline", "Align left": "justifyLeft", "Align center": "justifyCenter", "Align right": "justifyRight", "Align justify": "justifyFull", Bullets: "insertUnorderedList", "Numbered list": "insertOrderedList" };
    for (const [label, name] of Object.entries(names)) {
      const button = toolbar.querySelector(`[aria-label="${label}"]`);
      try { button?.setAttribute("aria-pressed", String(document.queryCommandState(name))); } catch { button?.setAttribute("aria-pressed", "false"); }
    }
    const selection = window.getSelection();
    let node = selection?.anchorNode;
    if (node?.nodeType === Node.TEXT_NODE) node = node.parentElement;
    if (node && editor.contains(node)) {
      const size = pendingSize !== null && selection.isCollapsed ? pendingSize : Math.round(parseFloat(getComputedStyle(node).fontSize));
      if (size >= 8 && size <= 72) {
        if (![...sizeInput.options].some(option => Number(option.value) === size)) {
          const option = element("option", String(size)); option.value = String(size);
          sizeInput.insertBefore(option, [...sizeInput.options].find(item => Number(item.value) > size) ?? null);
        }
        sizeInput.value = String(size);
      }
    }
  }
  function selectedDraft() {
    captureRange();
    if (savedRange && !savedRange.collapsed && editor.contains(savedRange.startContainer) && editor.contains(savedRange.endContainer)) {
      let selected = savedRange.cloneContents(), ancestor = savedRange.commonAncestorContainer;
      if (ancestor.nodeType === Node.TEXT_NODE) ancestor = ancestor.parentNode;
      // cloneContents omits formatting ancestors when selecting part of a run.
      while (ancestor && ancestor !== editor) { const wrapper = ancestor.cloneNode(false); wrapper.append(selected); selected = wrapper; ancestor = ancestor.parentNode; }
      const holder = document.createDocumentFragment(); holder.append(selected);
      return readDOM(holder);
    }
    return readDOM(editor);
  }
  function selectedText() {
    captureRange();
    if (savedRange && !savedRange.collapsed && editor.contains(savedRange.startContainer) && editor.contains(savedRange.endContainer)) {
      // A substring inside a list item is an excerpt, not a new numbered item.
      return plainText(readDOM(savedRange.cloneContents()));
    }
    return plainText(readDOM(editor));
  }
  async function copyDraft(draft, recoveredCopy = false, selected = null) {
    const text = selected ?? plainText(draft);
    if (!text && !draft.blocks?.length) { status("There is no text to copy."); return; }
    try {
      if (navigator.clipboard?.write && typeof ClipboardItem !== "undefined") {
        await navigator.clipboard.write([new ClipboardItem({ "text/plain": new Blob([text], { type: "text/plain" }), "text/html": new Blob([html(draft)], { type: "text/html" }) })]);
        status(recoveredCopy ? "Recovered text and formatting copied." : "Text and formatting copied.");
      } else {
        await navigator.clipboard.writeText(text); status(recoveredCopy ? "Recovered plain text copied." : "Plain text copied.");
      }
    } catch {
      try { await navigator.clipboard.writeText(text); status("Plain text copied."); }
      catch { status("Clipboard access was denied. Select your text and use Ctrl+C or your device's Copy command.", true); }
    }
  }
  function send(kind) {
    if (historyPending) return;
    try {
      publish(); const text = selectedText();
      if (!text.trim()) { status("Select or enter some text first."); return; }
      resetGroup(); enqueue({ kind, text });
    } catch (error) { status(error.message ?? String(error), true); }
  }
  function openLink() {
    if (historyPending) return;
    captureRange(); resetGroup();
    let node = savedRange?.startContainer;
    if (node?.nodeType === Node.TEXT_NODE) node = node.parentElement;
    linkInput.value = node?.closest?.("a")?.getAttribute("href") ?? "https://";
    linkRow.hidden = false; linkInput.focus(); linkInput.select();
  }
  function closeLink(refocus = true) { linkRow.hidden = true; if (refocus && visible) restoreRange(); }
  function applyLink() {
    const link = linkInput.value.trim();
    if (!safeLink(link)) { status("Use an http://, https:// or mailto: link without spaces.", true); linkInput.focus(); return; }
    restoreRange();
    if (window.getSelection()?.isCollapsed) {
      const anchor = element("a", link); anchor.href = link; anchor.rel = "noopener noreferrer"; anchor.target = "_blank";
      const holder = element("div"); holder.append(anchor); command("insertHTML", holder.innerHTML);
    } else command("createLink", link);
    for (const anchor of editor.querySelectorAll("a")) { anchor.rel = "noopener noreferrer"; anchor.target = "_blank"; }
    closeLink(); status("");
  }
  function insertClipboard(data, plain = false) {
    if (historyPending) return;
    resetGroup(); pendingSize = null;
    const rich = plain ? "" : data.getData("text/html"), text = data.getData("text/plain");
    try {
      let draft;
      if (rich) {
        if (bytes(rich) > pasteLimit) throw new Error("The pasted formatting exceeds 4 MiB. Paste a smaller selection or use Ctrl+Shift+V for plain text.");
        const template = element("template"); template.innerHTML = rich; draft = readDOM(template.content);
      } else draft = normalize({ blocks: text.replace(/\r\n?/g, "\n").split("\n").map(text => ({ runs: [{ text }] })) });
      if (!rich && !text) return;
      restoreRange();
      // Validate the combined document before mutating the user's editor.
      const trial = editor.cloneNode(true), selected = window.getSelection().getRangeAt(0);
      function boundary(node, offset) {
        const path = [];
        while (node !== editor) { path.unshift([...node.parentNode.childNodes].indexOf(node)); node = node.parentNode; }
        let copy = trial; for (const index of path) copy = copy.childNodes[index];
        return [copy, offset];
      }
      const start = boundary(selected.startContainer, selected.startOffset), end = boundary(selected.endContainer, selected.endOffset);
      const testRange = document.createRange(); testRange.setStart(start[0], start[1]); testRange.setEnd(end[0], end[1]); testRange.deleteContents(); testRange.insertNode(fragment(draft)); readDOM(trial);
      const inserted = html(draft); applying = true;
      try {
        if (!document.execCommand("insertHTML", false, inserted)) {
          const range = window.getSelection().getRangeAt(0); range.deleteContents();
          const nodes = fragment(draft), last = nodes.lastChild; range.insertNode(nodes);
          if (last) { range.setStartAfter(last); range.collapse(true); window.getSelection().removeAllRanges(); window.getSelection().addRange(range); }
        }
      } finally { applying = false; }
      captureRange(); publish("", true);
    } catch (error) { status(error.message ?? String(error), true); }
  }
  function requestHistory(kind) {
    if (historyPending || composing) return;
    publish();
    if (invalidLocal) { status("Finish or shorten the current text before using Undo or Redo.", true); return; }
    resetGroup(); captureRange(); historyFocus = editor.contains(document.activeElement) || document.activeElement === editor;
    enqueue({ kind }); historyPending = true; editor.contentEditable = "false";
  }
  function recoveryUI() {
    recoveryBanner.hidden = !recovered.length;
    recoveryBanner.querySelector("span").textContent = "A different unsaved Draft was preserved. Your saved Draft is shown below.";
  }
  function restoreRecovery(loaded) {
    try {
      const text = sessionStorage.getItem(recoveryKey);
      if (!text) return null;
      if (bytes(text) > recoveryLimit) throw new Error("Saved Draft recovery exceeds 4 MiB; the existing recovery copy was preserved.");
      const record = JSON.parse(text);
      if (record.version !== 1 || !Array.isArray(record.recovered ?? [])) throw new Error("Could not read this tab's Draft recovery; it has been preserved.");
      const base = normalize(record.base), draft = normalize(record.draft);
      recovered = (record.recovered ?? []).map(normalize);
      if (JSON.stringify(draft) === JSON.stringify(loaded)) return null;
      if (JSON.stringify(base) === JSON.stringify(loaded)) return draft;
      if (!recovered.some(item => JSON.stringify(item) === JSON.stringify(draft))) recovered.push(draft);
      recoveryUI(); return null;
    } catch (error) { recoveryBlocked = true; storageProblem = error.message ?? String(error); status(storageProblem, true); return null; }
  }
  function setup() {
    if (panel) return;
    panel = element("section"); panel.id = "jig-draft-panel"; panel.hidden = true; panel.setAttribute("aria-label", "Draft");
    toolbar = element("div", undefined, "jig-draft-toolbar"); toolbar.setAttribute("role", "toolbar"); toolbar.setAttribute("aria-label", "Draft formatting");
    function button(label, text, action, toggle = false) {
      const item = element("button", text); item.type = "button"; item.setAttribute("aria-label", label); item.title = label;
      if (toggle) item.setAttribute("aria-pressed", "false");
      item.addEventListener("mousedown", event => { event.preventDefault(); captureRange(); });
      item.addEventListener("click", action); toolbar.append(item); return item;
    }
    sizeInput = element("select"); sizeInput.setAttribute("aria-label", "Text size"); sizeInput.title = "Text size";
    for (const size of [8, 10, 12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 60, 72]) { const option = element("option", String(size)); option.value = String(size); sizeInput.append(option); }
    sizeInput.value = "16"; sizeInput.addEventListener("pointerdown", captureRange); sizeInput.addEventListener("change", resizeText); toolbar.append(sizeInput);
    button("Bold", "B", () => command("bold"), true).classList.add("draft-bold");
    button("Italic", "I", () => command("italic"), true).classList.add("draft-italic");
    button("Underline", "U", () => command("underline"), true).classList.add("draft-underline");
    button("Link", "Link", openLink); button("Remove link", "Unlink", () => command("unlink"));
    for (const [name, commandName] of [["left", "justifyLeft"], ["center", "justifyCenter"], ["right", "justifyRight"], ["justify", "justifyFull"]]) {
      const item = button(`Align ${name}`, undefined, () => command(commandName), true);
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.setAttribute("viewBox", "0 0 20 16"); svg.setAttribute("aria-hidden", "true"); svg.setAttribute("width", "18"); svg.setAttribute("height", "16");
      for (const [index, length] of (name === "justify" ? [16, 16, 16, 16] : [16, 10, 14, 8]).entries()) {
        const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
        const x = name === "center" ? (20 - length) / 2 : name === "right" ? 18 - length : 2;
        for (const [key, value] of Object.entries({ x1: x, x2: x + length, y1: 2 + index * 4, y2: 2 + index * 4, stroke: "currentColor", "stroke-width": 1.4 })) line.setAttribute(key, String(value));
        svg.append(line);
      }
      item.append(svg);
    }
    button("Bullets", "• list", () => command("insertUnorderedList"), true);
    button("Numbered list", "1. list", () => command("insertOrderedList"), true);
    button("Copy", "Copy", () => { try { copyDraft(selectedDraft(), false, selectedText()); } catch (error) { status(error.message, true); } });
    button("To note", "To note", () => send("note")); button("To task", "To task", () => send("task"));
    linkRow = element("div", undefined, "jig-draft-link"); linkRow.hidden = true;
    linkInput = element("input"); linkInput.type = "url"; linkInput.id = "jig-draft-link-url"; linkInput.setAttribute("aria-label", "Link URL"); linkInput.placeholder = "https://…";
    const apply = element("button", "Apply link"); apply.type = "button"; apply.addEventListener("click", applyLink);
    const cancel = element("button", "Cancel"); cancel.type = "button"; cancel.addEventListener("click", () => closeLink());
    linkInput.addEventListener("keydown", event => { if (event.key === "Enter") { event.preventDefault(); applyLink(); } else if (event.key === "Escape") { event.preventDefault(); closeLink(); } });
    linkRow.append(linkInput, apply, cancel);
    recoveryBanner = element("div", undefined, "jig-draft-recovery"); recoveryBanner.hidden = true;
    const recoveryCopy = element("button", "Copy recovered text"); recoveryCopy.type = "button";
    recoveryCopy.addEventListener("click", () => copyDraft({ blocks: recovered.flatMap(item => item.blocks ?? []) }, true));
    recoveryBanner.append(element("span"), recoveryCopy);
    message = element("div", undefined, "jig-draft-message"); message.hidden = true; message.setAttribute("role", "status");
    editor = element("div"); editor.id = "jig-draft-editor"; editor.contentEditable = "true"; editor.spellcheck = false;
    editor.setAttribute("role", "textbox"); editor.setAttribute("aria-multiline", "true"); editor.setAttribute("aria-label", "Draft text"); editor.setAttribute("data-placeholder", "Paste or write something to work on…");
    editor.addEventListener("input", event => { if (!applying) { replaceFontSizes(); publish(event.inputType); } });
    editor.addEventListener("beforeinput", event => {
      if (["historyUndo", "historyRedo"].includes(event.inputType)) { event.preventDefault(); requestHistory(event.inputType === "historyUndo" ? "undo" : "redo"); return; }
      if (!composing && event.cancelable && event.inputType === "insertText" && typeof event.data === "string") {
        const selected = window.getSelection()?.toString() ?? "";
        if (bytes(editor.textContent) - bytes(selected) + bytes(event.data) > textLimit) { event.preventDefault(); status("Draft text must be at most 512 KiB.", true); resetGroup(); }
      }
    });
    editor.addEventListener("compositionstart", () => { composing = true; });
    editor.addEventListener("compositionend", () => { composing = false; replaceFontSizes(); publish("insertCompositionText"); updateToolbar(); });
    editor.addEventListener("pointerdown", () => { resetGroup(); pendingSize = null; });
    editor.addEventListener("blur", () => { captureRange(); resetGroup(); if (!historyPending) publish(); });
    editor.addEventListener("paste", event => { event.preventDefault(); insertClipboard(event.clipboardData, plainPaste); plainPaste = false; });
    editor.addEventListener("dragover", event => event.preventDefault());
    editor.addEventListener("drop", event => { event.preventDefault(); if (event.dataTransfer.files.length) status("Draft accepts text. Paste the text you need from the file."); else insertClipboard(event.dataTransfer); });
    editor.addEventListener("dragstart", event => event.preventDefault());
    editor.addEventListener("click", event => { const anchor = event.target.closest?.("a"); if (anchor) { event.preventDefault(); if (event.ctrlKey || event.metaKey) window.open(anchor.href, "_blank", "noopener,noreferrer"); } });
    panel.addEventListener("keydown", event => {
      event.stopPropagation();
      if (event.defaultPrevented || event.isComposing || composing) return;
      if (event.key === "Escape") { event.preventDefault(); captureRange(); publish(); resetGroup(); document.getElementById("jig_canvas")?.focus({ preventScroll: true }); return; }
      const editing = event.target === editor || editor.contains(event.target);
      if (!editing) return;
      const control = (event.ctrlKey || event.metaKey) && !event.altKey, key = event.key.toLowerCase();
      if (control && key === "z") { event.preventDefault(); requestHistory(event.shiftKey ? "redo" : "undo"); return; }
      if (control && key === "y") { event.preventDefault(); requestHistory("redo"); return; }
      if (control && ["b", "i", "u"].includes(key)) { event.preventDefault(); command({ b: "bold", i: "italic", u: "underline" }[key]); return; }
      if (control && key === "k") { event.preventDefault(); openLink(); return; }
      if (control && key === "s") { event.preventDefault(); publish(); return; }
      if (control && event.shiftKey && key === "v") plainPaste = true;
      if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown", "Tab"].includes(event.key) || control && key === "a") { resetGroup(); pendingSize = null; }
    });
    panel.addEventListener("keyup", event => event.stopPropagation());
    document.addEventListener("selectionchange", () => { if (visible && !applying) { captureRange(); updateToolbar(); } });
    panel.append(toolbar, linkRow, recoveryBanner, message, editor); document.body.append(panel);
  }

  window.jigDraftInit = callback => { wake = typeof callback === "function" ? callback : () => {}; setup(); };
  window.jigDraftSync = (json, force, saved) => {
    setup();
    try {
      const incoming = normalize(JSON.parse(json)), incomingJSON = JSON.stringify(incoming);
      const initial = firstSync; firstSync = false;
      let restore = null;
      if (initial) { durable = incoming; durableJSON = incomingJSON; restore = restoreRecovery(incoming); }
      const hadHistory = historyPending; historyPending = false; editor.contentEditable = "true";
      if (force || initial || incomingJSON === currentJSON) {
        if (force || initial) {
          const bookmark = selectionBookmark(); applying = true;
          editor.replaceChildren(fragment(incoming)); savedRange = null;
          applying = false; current = incoming; currentJSON = incomingJSON; invalidLocal = false;
          if (force) { events = []; resetGroup(); pendingSize = null; linkRow.hidden = true; }
          if (visible && (hadHistory && historyFocus || document.activeElement === editor)) { editor.focus({ preventScroll: true }); restoreBookmark(bookmark); }
        }
      }
      if (saved) { durable = incoming; durableJSON = incomingJSON; }
      if (restore) {
        current = restore; currentJSON = JSON.stringify(restore); editor.replaceChildren(fragment(restore));
        enqueue({ kind: "change", draft: restore, coalesce: false }); status("Recovered your unfinished Draft from this tab.");
      }
      recoveryUI(); journal();
      if (hadHistory && historyFocus && visible) editor.focus({ preventScroll: true });
      historyFocus = false; updateToolbar();
    } catch (error) { status(error.message ?? String(error), true); }
  };
  window.jigDraftLayout = (x, y, width, height, dark, show, colors = "") => {
    setup();
    const next = !!show && width > 0 && height > 0;
    if (visible && !next) { captureRange(); resetGroup(); if (editor.contains(document.activeElement) || document.activeElement === editor) editor.blur(); }
    visible = next; panel.hidden = !next;
    if (!next) return;
    const layout = [x, y, width, height, !!dark].join(",");
    if (layout !== lastLayout) { panel.style.left = `${x}px`; panel.style.top = `${y}px`; panel.style.width = `${width}px`; panel.style.height = `${height}px`; panel.dataset.theme = dark ? "dark" : "light"; lastLayout = layout; }
    if (colors && colors !== lastPalette) {
      const palette = JSON.parse(colors);
      const names = ["bg", "surface", "text", "muted", "rule", "accent", "selected"];
      if (Array.isArray(palette) && palette.length === names.length
        && palette.every(color => Array.isArray(color) && color.length === 3
          && color.every(channel => Number.isInteger(channel) && channel >= 0 && channel <= 255))) {
        palette.forEach((color, index) => panel.style.setProperty(`--draft-${names[index]}`, `rgb(${color.join(",")})`));
        lastPalette = colors;
      }
    }
    if (focusRequested) { focusRequested = false; restoreRange(); }
  };
  window.jigDraftFocus = () => { setup(); if (visible) restoreRange(); else focusRequested = true; };
  window.jigDraftTake = () => { const next = events; events = []; return JSON.stringify(next); };
  window.jigDraftPending = () => events.length > 0 || currentJSON !== durableJSON || invalidLocal || !!storageProblem;
  window.jigDraftError = error => { setup(); status(String(error), true); };
  window.jigDraftResetRecovery = () => {
    try { sessionStorage.removeItem(recoveryKey); }
    catch (error) {
      recoveryBlocked = true; storageProblem = `Could not clear this tab's old Draft recovery. ${error.message ?? error}`;
      status(storageProblem, true); throw new Error(storageProblem);
    }
    recovered = []; recoveryBlocked = false; storageProblem = ""; recoveryUI();
  };
})();
