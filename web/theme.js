/* Optional browser-exposed system accent; never reads files or uses a network. */
(() => {
  "use strict";
  let cached = "", checked = -Infinity;
  const invalidate = () => { checked = -Infinity; };
  window.addEventListener("focus", invalidate);
  document.addEventListener("visibilitychange", invalidate);
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener("change", invalidate);
  window.jigSystemAccent = () => {
    if (!window.CSS?.supports("color", "AccentColor")) return "";
    const now = performance.now();
    if (now - checked < 1000) return cached;
    checked = now;
    const probe = document.createElement("span");
    probe.style.cssText = "position:fixed;visibility:hidden;pointer-events:none;color:AccentColor;color-scheme:light dark";
    document.body.append(probe);
    try {
      const value = getComputedStyle(probe).color;
      const match = /^rgba?\(\s*(\d+)\s*[, ]\s*(\d+)\s*[, ]\s*(\d+)/i.exec(value);
      cached = match ? "#" + match.slice(1, 4).map(channel => Number(channel).toString(16).padStart(2, "0")).join("") : "";
    } finally { probe.remove(); }
    return cached;
  };
})();
