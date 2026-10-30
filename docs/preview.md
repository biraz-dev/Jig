# Browser development preview

The egui/eframe browser dashboard now uses the same Rust calculator, length,
task, deadline and calendar modules as the CLI and Ratatui dashboard. It follows
the approved two-column composition on wide screens, stacks panels on narrow
screens, and can expand one instrument. CRUD forms stay inline. There is no
backend, account, telemetry or data upload.

A single crate remains sufficient. Default Cargo features build the native
`jig` binary. The `web` feature builds `jig-web`; terminal dependencies are
excluded on WASM. `src/web.rs` contains the egui adapter, while
`src/browser_storage.rs` and `web/bridge.js` handle browser files and storage.
The shared core has no DOM, filesystem or terminal API. Trunk builds the root
`index.html` with the `wasm32-unknown-unknown` target.

## Build and use locally

From the development checkout on `jig-initial-milestone`:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
trunk serve --public-url /
```

Open the local address printed by Trunk. JavaScript, WebAssembly and WebGL must
be available. The native CLI and TUI remain usable independently of this target.
For a production artifact under the repository's Pages path, run:

```sh
trunk build --release --public-url /Jig/
```

Trunk writes the static site to `dist/`. Fonts ship with the application; no
remote font service is used. After the static assets load, utility operations,
clock updates, imports and exports run on the device. There is no service worker
or guaranteed offline asset cache. A local static server or the native binary is
appropriate when dependable offline startup is needed.

## Sample and personal data

Fresh profiles display **Development preview — sample data**. The synthetic
sample constructor uses a supplied clock and labels every task, deadline and
calendar note as a sample. Calculator examples also come from code. Demo edits
are temporary and never write the saved personal dataset.

**Start empty** asks before creating or replacing personal data. **Open local
data** returns to the saved personal dataset; **Sample mode** returns to a new
temporary demo. The header always distinguishes samples from **your local data**.
Successful personal edits save to localStorage key `jig.personal.v1` in this
browser profile and origin. Another computer, browser or profile has independent
storage.

**Import JSON** reads a local file, validates a complete Jig state, displays its
item counts and asks for confirmation before replacing the personal dataset.
**Export JSON** downloads the displayed dataset as `jig.json`, using the same
versioned JSON format as the CLI. Users can move their work between the browser
and `jig --state /path/to/jig.json` without an account. Importing or exporting does
not send application data to GitHub or a server.

Browser saves and imports are limited to 2 MiB. Invalid saved data is reported
and preserved. A failed save leaves edits in memory with a retry control; export
before closing the page. Before a save or clear, the bridge compares the current
stored text with the last observed copy. If another tab changed it, the operation
is rejected instead of overwriting that copy. Export current work, then reload
the page to open the newer saved version; there is no automatic merge.

**Clear saved data** requires inline confirmation, removes this preview's saved
personal dataset, and returns to temporary samples. Browser site-data clearing
can also remove it. JSON downloads are user-owned backups; localStorage is not a
backup service.

## Browser controls

Use the numbered tool buttons to focus an instrument. F2 toggles the expanded
view; Esc returns to all tools. Ctrl-Alt-4 focuses calculator input, and
Ctrl-Alt-N opens the start-empty confirmation. Tab/Shift-Tab navigate controls;
normal mouse and text editing work. Calculator history results can be clicked
to reuse their full precision. Task arrows reorder the checklist, deadline stars
choose the featured countdown, and calendar date buttons show local notes and
deadlines. Times display in the device's timezone; deadline input requires a
qualified RFC3339 time.

## GitHub Actions and Pages

The **Browser preview** workflow runs WASM Clippy, builds using pinned Trunk
0.21.14, inspects the static artifact, and exercises the real browser application
on every push and pull request. Browser test results and the static Pages
artifact are available in Actions. The separate Rust workflow retains native
formatting, Clippy, core tests, Linux PTY checks and release builds.

Deployment runs only after a push to `main`; pull requests build and test without
publishing. To activate the public preview:

1. In repository **Settings → Pages → Build and deployment**, choose
   **GitHub Actions** as the source.
2. Merge the reviewed milestone PR into `main`.
3. Check the **Browser preview** deploy job and use its reported Pages URL.

The repository's default branch still contains the initial README until the
milestone is merged. This document does not claim a completed deployment or a
live preview URL.

Only `dist/` is uploaded as the Pages artifact. Trunk copies the intended static
bootstrap assets and compiled application, not state files, home directories,
local uploads or private files. The workflow grants `pages: write` and
`id-token: write` only to the deployment job using the `github-pages` environment.
The public sample dataset is assembled in code and contains no personal state.

## Follow-up work

Check keyboard use, imports, browser storage recovery and resizing across real
browsers and devices. The egui canvas has limited screen-reader accessibility;
improving that is a priority before calling the preview broadly accessible.
Background tabs may throttle countdown redraws, so the display derives its value
from the current clock when it runs again. Improve narrow layouts and resolve
observed issues before adding more utilities. A native graphical interface,
service worker, synchronization service and additional tools are not part of
this preview.

`Ctrl+Alt+O` reopens the saved local dataset. If it cannot be decoded, normal edits
remain in memory and do not overwrite the saved raw data; export your work before
an explicitly confirmed replacement.
