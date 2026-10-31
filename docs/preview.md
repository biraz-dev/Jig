# Browser development preview

The egui/eframe browser dashboard now uses the same Rust calculator, length,
task, deadline and calendar modules as the CLI and Ratatui dashboard. It follows
the approved dark terminal aesthetic with a compact featured countdown, one
checklist of tasks with optional dates, a small month grid and a shared math
strip. Applets stack and scroll on narrow screens, and one can be maximized.
Calculator and length conversion share one input, result display and history.
Forms and Data controls stay inline. There is no
application backend, account or telemetry. Optional weather uses Open-Meteo;
only the chosen location and weather query are sent, never workbench records.

The STL applet adds a local 3D instrument beside math on larger screens and below
it on smaller screens. **Ctrl-Alt-5** selects it; the existing highlight, arrow
navigation and Ctrl-M maximize behavior apply. Its GPU renderer uses an offscreen
WebGL 2 color/depth framebuffer, then composites into the applet's clipped egui
rectangle. A corner X/Y/Z gnomon tracks orientation. Camera zoom is separate from
physical scale, and the viewer does not animate while idle. Its depth-buffered
texture is reused until the model, camera or viewport size changes.

`src/stl.rs` parses bounded binary/ASCII STL and computes bounds, area, topology
diagnostics and conditional volume without terminal, DOM or GPU dependencies.
`src/stl_ui.rs` handles the browser camera, renderer and measurement controls;
`web/stl.js` reads chosen files into memory and prepares local downloads. STL
bytes and filenames never enter browser storage, workbench JSON or a network
request. A bad replacement file preserves the current model. The CLI's `stl`
command uses the same geometry and scale calculations.

`src/clay.rs` contains the two exact published Standard #104 cone points and
validates user test profiles. Optional `Preferences.clay` stores only profile
selection and custom values in existing version-1 JSON. The manufacturer's
starting stage is unspecified and remains labelled as such. The estimate applies
uniform linear scaling; it does not model a kiln or interpolate cones. See the
README for the source, formulas and mesh-validity limits. No dependencies were
added for this applet.

A single crate remains sufficient. Default Cargo features build the native
`jig` binary. The `web` feature builds `jig-web`; terminal dependencies are
excluded on WASM. `src/web.rs` contains the egui adapter, while
`src/browser_storage.rs` and `web/bridge.js` handle browser files and storage.
The shared core has no DOM, filesystem or terminal API. Trunk builds the root
`index.html` with the `wasm32-unknown-unknown` target.

## Build and use locally

From a checkout of the repository:

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

Fresh profiles display a clearly labelled **Sample data** indicator. The synthetic
sample constructor uses a supplied clock and labels every task, deadline and
calendar note as a sample. Calculator examples also come from code. Demo edits
are temporary and never write the saved personal dataset.

Open **Data** (or Ctrl-Alt-D) for the dataset controls. **Start empty** asks before
creating or replacing personal data. **Open local
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

Click an applet to highlight it. Outside text editors, the arrow keys move the
highlight between applets. Ctrl-M maximizes or restores the selection; Esc leaves
an editor and returns to the dashboard. Ctrl-Alt-4 focuses math input, and
Ctrl-Alt-N opens the start-empty confirmation. Tab/Shift-Tab navigate controls;
normal mouse and text editing work. Type `125 * 1.15` for arithmetic or
`72 in mm` for a length conversion. History results can be clicked to reuse their
full precision, including the source unit of a converted result. Task arrows
reorder the checklist. Tasks may have a due date; a star chooses the featured
countdown and reveals its date in the calendar. Starring an undated task opens
its date editor first. Completing a task clears its star and subdues its calendar mark;
removing a task also removes its linked date. Existing standalone deadlines
appear in the same list without a migration or save merely to display them.

The compact calendar contains day numbers with today and small marks on due
dates only. Dated checklist tasks have an inline color selector with five muted,
named choices (Rose, Amber, Sage, Blue and Lilac) plus neutral. Multiple same-day
tasks have separate marks; a count indicates overflow. Completed marks are
subdued and hollow. Selecting a day by pointer, touch or Tab then Enter/Space
shows its task names and explicit Open/Done status in a compact scrolling list
within the calendar panel, alongside the existing selected-day weather. This
never changes the starred task or featured countdown. Date, color and completion
changes are reflected from the shared task/deadline links immediately.

`Task.color` is an optional named value in the existing version-1 JSON format.
Missing colors default to neutral, unknown colors are rejected on import, and
colors survive completion and temporarily removing a due date. The calendar is
a read-only projection, not another persisted set of task records. Legacy
standalone deadlines remain visible without creating tasks on load or selection.
Entry forms and markers are hidden, while existing entries
remain intact in stored state and JSON exports. Times display in the device's
timezone; deadline input requires a qualified RFC3339 time.

The hybrid input also accepts `72in to mm`, `72in -> mm`, `72in → mm`, and mixed
fractions such as `1 1/2 in cm`. There is one saved record per successful
submission. Target units are stored with converted results; old history records
remain compatible. `ans` is the last numeric answer, and `ans in mm` supplies an
explicit source unit for a conversion. Decimal places and **Copy** are inside the
same instrument. Selection highlighting and Ctrl-M replace open/close controls.
The browser reserves Ctrl-M only while the Jig canvas or its text input has focus.

## GitHub Actions and Pages

The **Browser preview** workflow runs WASM Clippy, builds using pinned Trunk
0.21.14, inspects the static artifact, and exercises the real browser application
on every push and pull request. Browser test results and the static Pages
artifact are available in Actions. The separate Rust workflow retains native
formatting, Clippy, core tests, Linux PTY checks and release builds.

Screenshots normally stay in the test artifact. A push whose commit message
contains `[render]` also prints allowlisted synthetic screenshots as
bounded PNG data for visual review. Set `JIG_RENDER_LOGS` when running the test
locally to request named scenes; personal workbench data is never used.

`tests/stl_browser.mjs` runs separately from the general dashboard suite. It uses
synthetic cube/open meshes and the real local file picker bridge, exercises
measurement reports and downloads, and checks camera/scale controls, file error
preservation and reload privacy. The public demo never embeds a private STL.

Deployment runs only after a push to `main`; pull requests build and test without
publishing. To activate the public preview:

1. In repository **Settings → Pages → Build and deployment**, choose
   **GitHub Actions** as the source.
2. Merge the reviewed milestone PR into `main`.
3. Check the **Browser preview** deploy job and use its reported Pages URL.

The expected repository URL is `https://biraz-dev.github.io/Jig/`; the deployment
job is the authority for its publication status.

Only `dist/` is uploaded as the Pages artifact. Trunk copies the intended static
bootstrap assets and compiled application, not state files, home directories,
local uploads or private files. The workflow grants `pages: write` and
`id-token: write` only to the deployment job using the `github-pages` environment.
The public sample dataset is assembled in code and contains no personal state.
Weather is disabled by default; no personal location is embedded in the bundle.

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

## Optional weather

The calendar uses the shared `weather` model and browser-only `weather_client`
and `weather_ui` adapters. Open-Meteo forecasts and city/postcode search require
no account or key for non-commercial use. Both Open-Meteo and GeoNames attribution
are shown beside the controls. See the README for provider terms and limits.

Location and °F/°C can be changed in the inline location control. Browser settings
use `jig.weather.settings.v1`; normalized forecasts use `jig.weather.cache.v1`.
The chosen preference also travels in personal workbench JSON. Fresh matching
city/unit caches are reused for an hour. Failed refreshes keep matching cached
forecasts with a stale/error label and do not retry on each dashboard repaint.
Changing or disabling weather invalidates pending requests, so a late response
cannot restore a cleared city. Clearing saved app data clears weather too.

No outgoing weather request contains tasks, history or deadlines. API fetches
omit credentials and referrer information, use a bounded timeout, and are never
started for a plain unconfigured dashboard. A city search is a deliberate network
action. Offline viewing can use cached forecasts; the timestamps make their age
visible. Monthly dates beyond the provider's forecast horizon have no weather.

Browser tests intercept the provider with synthetic data to check Fahrenheit,
Celsius, rain probability, humidity normalization, cache reuse, stale data and
bounded failure retries without depending on provider availability in CI.
