// Observe real egui paints rather than assuming a DOM animation callback ran
// the app. This probe is installed only by browser tests, never by the product.
export async function installFrameFence(context) {
  await context.addInitScript(() => {
    const state = { input: 0, processed: -1, painted: -1 };
    window.jigTestFrames = state;
    const app = target => target?.id === "jig_canvas" || (target instanceof HTMLInputElement
      && target.type === "text" && !target.id && target.parentElement === document.body);
    for (const type of ["keydown", "keyup", "beforeinput", "input", "copy", "cut", "paste", "pointerdown", "pointerup", "touchstart", "touchend"]) {
      document.addEventListener(type, event => {
        if (app(event.target)) state.input++;
      }, true);
    }
    // eframe 0.31 sets the body's cursor after every app.update(), including
    // updates that prepare a frame for painting on the following animation tick.
    const setProperty = CSSStyleDeclaration.prototype.setProperty;
    CSSStyleDeclaration.prototype.setProperty = function (name, value, priority) {
      const result = setProperty.call(this, name, value, priority);
      if (this === document.body?.style && name === "cursor") state.processed = state.input;
      return result;
    };
    for (const Context of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
      if (!Context) continue;
      const clear = Context.prototype.clear;
      Context.prototype.clear = function (mask) {
        const result = clear.call(this, mask);
        // egui clears the default target with color only. STL clears its own
        // framebuffer with color/depth and must not count as another UI frame.
        if (this.canvas?.id === "jig_canvas" && mask === this.COLOR_BUFFER_BIT
          && this.getParameter(this.FRAMEBUFFER_BINDING) === null) state.painted = state.processed;
        return result;
      };
    }
  });
}

export async function waitForAppPaint(page) {
  // Painting a previously prepared frame must not acknowledge newer input.
  await page.waitForFunction(() => window.jigTestFrames
    && window.jigTestFrames.painted >= window.jigTestFrames.input,
    undefined, { timeout: 10000, polling: 25 });
}
