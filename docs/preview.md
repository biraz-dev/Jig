# Browser development preview

The egui/eframe browser application opens Tasks by default and remembers the
selected workspace. The top bar switches between Tasks, Notes and Sheet;
calculator, Undo, Redo and Data controls sit on the right. The title and tagline
no longer occupy header space. Sheet retains its named tabs, formulas,
formatting and charts. An older saved Workbench preference opens Tasks.
Tasks and task Gantt charts read the same records;
they are never copied into a second task store. ODS is the portable document,
CSV exchanges evaluated cells, and PDF prints sheets and chart pages. See the
[spreadsheet guide](../README.md#spreadsheet) for controls and limits.

The warm Graphite palette uses charcoal surfaces, soft warm white text and a
blood-orange red focus color. Shared RGB constants keep browser and terminal
chrome consistent, including the loading screen. Task and chart category colors, explicit cell formatting and light document exports retain
their own colors. The matching SVG favicon has an opaque charcoal interior.

Sheet formatting includes left, center and right alignment, an Auto reset,
strikeout and range-aware border presets. Row/column headers have insert/delete
context menus, also available by touch-hold or Shift-F10 on selected headers.
Sheet add/delete controls sit alongside the tabs; double-click a tab to rename.
USD is the only currency offered for new styles. Existing imported currency
metadata is preserved. Standard ODS and PDF use the same cell styles as the grid.
The formatting toolbar uses drawn alignment icons and a compact border preview
grid with solid affected edges and dashed guides. Each control keeps a named
tooltip, focus feedback and keyboard activation; no icon font is required.

Spreadsheet `CONVERT(value,"cm","in")` uses the same Rust length scales as the
calculator popover and CLI. Square and cubic units support area and volume
calculations, with cell references and full calculation precision. See the
[conversion examples](../README.md#conversion-formulas) for measurements and
cost formulas. Invalid units and mixed dimensions remain visible errors.

The Tasks board uses the same Rust task, deadline and calendar records as the
CLI and Ratatui dashboard. Its checklist and Gantt share a resizable panel, with
notes below and a compact featured countdown above. The separate Workbench tab
is removed. A calculator icon beside Undo opens the shared arithmetic/conversion
input, result, precision control and history without leaving the workspace.
Click outside or press Esc to close the popover. There is no application backend,
account or telemetry. Optional weather uses Open-Meteo; only the chosen location
and weather query are sent, never task, note or document contents.

The browser STL interface and its entry shortcuts are hidden for now. The Rust
geometry, scale, camera and clay-estimate modules remain, along with the native
`jig stl` command and its tests. Existing saved clay preferences are preserved;
this UI change does not delete or move data to another application. See the
[CLI geometry notes](../README.md#stl-and-clay-calculations) for measurement and
shrinkage limits.

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
python3 scripts/fetch_ocr_assets.py
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

Fresh profiles start with an empty personal document; saved work opens on return.
Choose **Sample mode** or append `?sample=1` for a clearly labelled demo. The synthetic
sample constructor uses a supplied clock and labels every task, deadline and
calendar note as a sample. Calculator examples also come from code. Demo edits
are temporary and never write the saved personal dataset.

Open **Data** (or Ctrl-Alt-D) for the dataset controls. **Start empty** asks before
creating or replacing personal data. **Open local
data** returns to the saved personal dataset; **Sample mode** returns to a new
temporary demo. The wide header distinguishes samples from **your local data**;
the status message also identifies temporary sample mode.
Successful personal edits save to localStorage key `jig.personal.v1` in this
browser profile and origin. Another computer, browser or profile has independent
storage.

Normal refreshes and deployed application updates retain that saved dataset.
Unfinished task/note text and date edits are backed up per tab in session storage and restored
after refresh; changed underlying records are recovered as new drafts rather than
overwritten. Save/Ctrl-Enter commits a note, and Enter adds a task. Workspace
preferences are stored separately in this browser.
Commit a sheet editor with Enter before leaving. Existing sample-mode edits can
be kept by exporting JSON and importing that file as personal data; Start empty
deliberately starts an empty dataset.

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

The wide Tasks workspace has a draggable separator between the task list and
timeline. It has a horizontal resize cursor and grip,
keyboard Left/Right adjustment (Shift for larger steps), and double-click reset.
Resizing commits one preference change at the end of the drag; it does not write
or add Undo entries for each mouse movement. The optional task-panel percentage
is saved in the existing preferences, including JSON and Jig ODS backups. Older
documents omit it and use the original layout. Panel minimum widths adapt to the
available space, and the narrow stacked layout preserves the preferred split.

Click the task or calendar/timeline panel to highlight it. Outside text
editors, arrows move between panels. Ctrl-M maximizes or restores the selected
panel. Tab/Shift-Tab navigate controls; mouse and touch operate the same controls.
Ctrl-Alt-2 opens Tasks, Ctrl-Alt-N opens Notes and Ctrl-Alt-W switches Sheet/Tasks.
Ctrl-Alt-D opens Data controls. Popup controls own their arrows and Escape.

The calculator icon, or Ctrl-Alt-4, opens a compact popover in the current
workspace. Type `125 * 1.15` for arithmetic or `72 in mm` for a length conversion.
History results reuse full precision, including the source unit of a converted
result. Decimal places and Copy stay inside the popover. No separate units
applet is needed.

Task arrows and grips reorder the checklist. Sort orders the selected lists by
earliest due date, keeps undated tasks in their current order beneath dated
tasks, and moves completed tasks to the bottom. Equal due dates retain their
order. The Gantt follows the same saved order; one Undo restores it. Sorting is
explicit, so manual ordering still works afterward. Sort can also link older
standalone deadline rows on request, reusing their existing deadline IDs.
Tasks may have a due date; a star chooses the featured countdown and reveals its
date in the calendar. Starring an undated task opens its date editor first.
Completing a task clears its star and subdues its calendar mark; removing a task
also removes its linked date. Existing standalone deadlines appear without a
migration or save merely to display them.

The calendar is a continuous date strip with month-boundary labels. Widening the
panel reveals more dates, including dates in neighboring months. Drag the strip
or use horizontal/Shift-wheel scrolling to pan; +/−, Ctrl-wheel or a pinch zooms
the day width. Today returns to the current day. When the strip has keyboard
focus, Left/Right select a day, Page Up/Down move by a visible page, Home selects
today, and +/− zoom. Panning and zooming do not edit task dates.

Dated checklist tasks have an inline color selector with five muted, named
choices (Rose, Amber, Sage, Blue and Lilac) plus neutral. The same colors mark
their due day only. Multiple same-day tasks have separate marks; a count indicates
overflow. Completed marks are subdued and hollow. Click or tap a date, or select
it with the keyboard, to show task names and explicit Open/Done status in a
compact list within the panel. The selected lists determine the visible task
marks and details. Date selection never changes the starred task or featured
countdown. Date, color and completion changes follow the shared links.

Weather controls and a compact forecast strip sit above Calendar/Timeline. The
strip shows today's available forecast by default; select a calendar day by
pointer, touch or keyboard to see that date instead. This selection is temporary
UI state and does not change saved records. No weather footer takes space beneath
the chart.

`Task.color` is an optional named value in the existing version-1 JSON format.
Missing colors default to neutral, unknown colors are rejected on import, and
colors survive completion and temporarily removing a due date. The calendar is
a read-only projection, not another persisted set of task records. Legacy
standalone deadlines remain visible without creating tasks on load or selection.
Entry forms and markers are hidden, while existing entries
remain intact in stored state and JSON exports. Times display in the device's
timezone; date fields accept calendar dates, local date/time or qualified
RFC3339 timestamps.

The **Calendar / Timeline** selector reuses this panel; Ctrl-Alt-T switches views
and focuses Timeline's Week control or the calendar date strip as appropriate.
The compact Gantt adapter in `src/timeline_ui.rs` draws day-based Week/Month ranges from the portable
`src/timeline.rs` projection. Existing task colors and completion status appear
on bars, with deadline-only diamonds and explicit No date rows. Tab, Enter/Space,
pointer and touch open an inline editor. Selection is independent of featuring.
The checklist and timeline share the same date editor and global Undo/Redo
journal. The calendar icon beside either date field opens a month picker with a
local 24-hour HH:MM time. Browsing and selecting days only change the picker;
Use date updates the draft, Save date commits the range, and Cancel leaves the
stored schedule untouched.

To choose dates on the main strip, keep the task's date editor open, switch to
Calendar, and click or focus Start or Due. A label shows which field the next
selected day will fill. The chosen time is retained, and Save date commits the
whole range. Done picking or Esc leaves picking mode without discarding the
draft. Panning and zooming only move the view; the featured countdown changes
only when a saved edit changes its task's schedule.

Inside the timeline, a draggable title/grid divider saves the optional
`Preferences.timeline_label_percent` (10–80); absent values retain the original
layout. Pointer moves preview a width and release commits one Undo step.
Double-click resets it; focused Left/Right and Shift arrows adjust it by 1 or 5
percentage points. This preference is independent of the outer Tasks/timeline
split, and both travel in JSON and embedded ODS state.

Ranged task bars have start/due handles with day-snapped previews, release-only
saves, Escape cancellation and keyboard changes of one day or Shift for a week.
Only real visible endpoints have handles; clipped edges and deadline-only
diamonds cannot be resized. The portable `timeline::resize_schedule` proposes a
validated range, preserving the opposite instant and the moved end's local
clock, including exact no-ops during repeated DST hours. Crossings clamp to a
one-day range; nonexistent local times are rejected. The existing schedule
setter updates the original linked records. No duplicate task, deadline or
selection change is created. Dataset, viewport, range and row changes cancel
stale gestures; the browser tests cover both ends, live preview, Undo and reload.

`Task.start` is an optional UTC instant, omitted when absent. New date-only
starts use local midnight and new date-only deadlines use 09:00. Moving an
existing start or due day retains its local time. An unchanged displayed local
time retains the exact stored instant, including seconds, subseconds and a
repeated DST hour. Nonexistent local times are rejected with a visible error;
explicit RFC3339 timestamps remain available. Whole-range validation precedes
assignment. Legacy records are linked only on a successful explicit save, and
passive views do not migrate state. JSON remains version 1, without added
dependencies. Core tests cover precise time resolution, atomic edits, viewport
math and clipping; browser suites check editing, history, persistence and mobile
use.

The hybrid input also accepts `72in to mm`, `72in -> mm`, `72in → mm`, and mixed
fractions such as `1 1/2 in cm`. There is one saved record per successful
submission. Target units are stored with converted results; old history records
remain compatible. `ans` is the last numeric answer, and `ans in mm` supplies an
explicit source unit for a conversion. Decimal places and **Copy** are inside the
same popover. Click outside or press Esc to close it. Ctrl-M remains reserved for
the Tasks/calendar panels while the Jig canvas or its text input has focus.

## GitHub Actions and Pages

The **Browser preview** workflow runs WASM Clippy, builds using pinned Trunk
0.21.14, inspects the static artifact, and exercises the real browser application
on every push and pull request. Browser test results and the static Pages
artifact are available in Actions. The separate Rust workflow retains native
formatting, Clippy, core tests, Linux PTY checks and release builds.
The seven browser suites run against the same built artifact in separate jobs,
with at most six runners in parallel. Pages deploys only after all suites pass.

Screenshots normally stay in the test artifact. A push whose commit message
contains `[render]` also prints allowlisted synthetic screenshots as
bounded PNG data for visual review. Set `JIG_RENDER_LOGS` when running the test
locally to request named scenes; personal application data is never used.

The deployment gates cover spreadsheets, local OCR, the timeline, task lists,
notes, general browser flows and Undo/Redo. The four STL-only browser suites are
not deployment gates while that interface is hidden. Native geometry, scale,
camera, comparison and shrinkage tests still run in `cargo test`.

The header Undo/Ctrl-Z and Redo/Ctrl-Shift-Z share one chronological journal for
spreadsheet, task, note, calculator and preference changes. Undo/Redo navigation
moves snapshots between stacks; a new committed edit clears Redo and a no-op
keeps it. Twenty combined steps and a shared 64 MiB budget bound history. Data
restores use the normal safe-save path and update weather when its preference
changes. Draft guards prevent history navigation from discarding unfinished
work. Dataset switches and reload clear this session journal; it is not included
in JSON or ODS. Text editors retain their normal local editing shortcuts.
`tests/undo_browser.mjs` covers history branching and dataset/save protections.

Deployment runs only after a push to `main`; pull requests build and test without
publishing. To activate the public preview:

1. In repository **Settings → Pages → Build and deployment**, choose
   **GitHub Actions** as the source.
2. Merge the reviewed milestone PR into `main`.
3. Check the **Browser preview** deploy job and use its reported Pages URL.

The public app is `https://biraz.dev/Jig/`; the deployment job checks the live
release marker and WebAssembly asset. The account's landing-page repository owns
the domain. Keep Jig's custom-domain setting empty and its repository free of CNAME files.

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
The chosen preference also travels in personal Jig JSON. Fresh matching
city/unit caches are reused for an hour. Failed refreshes keep matching cached
forecasts with a stale/error label and do not retry on each dashboard repaint.
Changing or disabling weather invalidates pending requests, so a late response
cannot restore a cleared city. Clearing saved app data clears weather too.

No outgoing weather request contains tasks, history or deadlines. API fetches
omit credentials and referrer information, use a bounded timeout, and are never
started for a plain unconfigured dashboard. A city search is a deliberate network
action. Offline viewing can use cached forecasts; the timestamps make their age
visible. Dates beyond the provider's forecast horizon have no weather.

Browser tests intercept the provider with synthetic data to check Fahrenheit,
Celsius, rain probability, humidity normalization, cache reuse, stale data and
bounded failure retries without depending on provider availability in CI.
