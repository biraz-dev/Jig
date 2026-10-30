# Browser development preview

The egui/eframe browser dashboard now uses the same Rust calculator, length,
task, deadline and calendar modules as the CLI and Ratatui dashboard. It follows
the approved two-column composition on wide screens, stacks panels on narrow
screens, and can expand one instrument. Calculator and length conversion share
one panel, with the converter taking half its former width. Task and deadline
forms stay inline. There is no
application backend, account or telemetry. Optional weather uses Open-Meteo;
only the chosen location and weather query are sent, never workbench records.

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

Use each panel's **open** / **close** buttons. F2 toggles the focused instrument
between expanded and dashboard views; Esc returns to all tools. Ctrl-Alt-4 focuses calculator input, and
Ctrl-Alt-N opens the start-empty confirmation. Tab/Shift-Tab navigate controls;
normal mouse and text editing work. Ctrl-Alt-U transfers the last calculator
result into the converter without rounding. Calculator history results can be clicked
to reuse their full precision. Task arrows reorder the checklist, deadline stars
choose the featured countdown and reveal its date in the calendar. The calendar
is a month grid only: today is highlighted, deadline dates are accented, and
clicking a deadline date features its earliest deadline. Hover shows matching
deadline titles. Entry forms and markers are hidden, while existing entries
remain intact in stored state and JSON exports. Times display in the device's
timezone; deadline input requires a qualified RFC3339 time.

The **Use result** button or Ctrl-Alt-U transfers the last calculator result into
the converter's current source unit without rounding. Decimal places are adjusted
inside that combined tool. It does not add a history
record. The **open** / **close** buttons expand an instrument and return to the
four-panel dashboard.

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
