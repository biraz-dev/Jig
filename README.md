# Jig

A tool for creative Small Giants. A small, local artist's workbench in Rust:
calculator, length conversion, tasks, deadlines and calendar. The Rust CLI,
terminal dashboard and egui browser preview share the same utility logic.
No accounts, telemetry, database, backend or background service. The CLI and
terminal run offline; the browser loads static application assets and processes
all application data on your device.

![Jig terminal dashboard — sample data](docs/sample-dashboard.svg)

The image above renders the Ratatui dashboard with clearly labelled sample data.
The browser preview uses the same five instruments and the approved visual
direction.
The [browser preview](https://biraz-dev.github.io/Jig/) is deployed from `main`
through GitHub Actions. Fresh visitors see clearly labelled sample data.

## Build and run on Linux

Install the stable Rust toolchain with rustup, then:

```sh
git clone https://github.com/biraz-dev/Jig.git
cd Jig
cargo build --locked --release
./target/release/jig
# Optional: install the binary in ~/.cargo/bin
cargo install --locked --path .
```

A UTF-8 terminal is recommended. Set IBM Plex Mono in your terminal preferences
if installed; terminal applications cannot select the font. The complete tiled
dashboard needs at least 100 columns × 36 rows. Smaller terminals show the focused
tool; below 32 × 10 Jig asks you to resize. F2 expands a tool at any size.

## Run the browser preview locally

Install stable Rust and the pinned Trunk build tool, then run from the repository:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
trunk serve --public-url /
```

Open the local address printed by Trunk, normally `http://127.0.0.1:8080/`.
The preview needs JavaScript, WebAssembly and WebGL enabled. Native terminal
dependencies are excluded from the WASM build; `jig` still launches the TUI by
default.

A fresh browser opens **Development preview — sample data**. Demo edits are
temporary and never replace saved personal data. **Start empty** creates your own
workbench after confirmation; **Open local data** returns to saved personal data;
**Sample mode** returns to the demo. Personal edits save in this browser profile
under `jig.personal.v1`. **Import JSON** validates a complete Jig state file and
asks before replacing personal data; **Export JSON** downloads the current
workbench as `jig.json`. **Clear saved data** requires confirmation and returns to
temporary samples. Use export for backups and to transfer data between the
browser and CLI. Importing a file never uploads it.

The preview supports mouse interaction and normal text editing. Use the numbered
tool buttons to focus an instrument, **F2** to expand it, and **Esc** to return to
the dashboard. **Ctrl-Alt-4** focuses the calculator input; **Ctrl-Alt-N** opens the
start-empty confirmation; **Ctrl-Alt-O** reopens local data. Tab/Shift-Tab navigate controls. On narrow screens the
panels stack vertically; inline forms and individual panels can scroll.

Browser imports and saves have a 2 MiB limit. Invalid saved data is preserved and
reported. If a save fails, edits remain in memory and **Retry save** becomes
available; export them before closing the page. If another tab changes the saved
text, Jig rejects the stale write. Export your current work, then reload to open
the newer saved copy. Browser storage belongs to this browser profile and origin;
clearing site data or using another browser does not carry it over.

For a production build under the repository's Pages path:

```sh
trunk build --release --public-url /Jig/
```

The **Browser preview** Actions workflow builds on pushes and pull requests,
checks WASM with Clippy, exercises the browser app, and uploads static assets and
browser test results. Deployment runs only on a push to `main`. To make the site
public, set **Settings → Pages → Build and deployment → Source → GitHub Actions**
before the first deployment. The workflow reports the published URL. See
[the browser preview notes](docs/preview.md) for implementation and deployment
details.

## CLI

```sh
jig calc '125 * 1.15'
jig calc '(12 + 8) * 50%'
jig calc 'ans / 2'
jig history
jig units 12in mm
jig units '1 1/2in' cm
jig units 304.8mm ft
jig tasks
jig tasks add 'Photograph sculptures'
jig tasks edit 1 'Photograph new sculptures'
jig tasks done 1
jig tasks move 1 2
jig tasks delete 1
jig deadline add 'Exhibition opening' '2026-12-01T18:00:00+01:00'
jig deadline list
jig deadline feature 2
jig deadline countdown
jig deadline edit 2 'Opening' '2026-12-02T18:00:00+01:00'
jig deadline delete 2
jig calendar
jig calendar --year 2027 --month 1
jig calendar add 2026-12-01 'Studio visit'
jig calendar list
jig calendar edit 3 2026-12-02 'Workshop'
jig calendar delete 3
jig precision 3
jig --state /path/to/portable/jig.json
jig --help
```

IDs are printed when adding items and listed by CLI commands. They are shared
across tasks, deadlines and calendar notes; use the actual returned IDs.
`tasks done` toggles completion. Task positions are 1-based. Errors go to stderr
with a nonzero exit status. `deadline countdown` prints one snapshot; the TUI
refreshes the countdown every second.

Calculator syntax: `+ - * /`, decimals, unary signs, parentheses, postfix `%`,
and `ans` (the last result). Percent always divides its operand by 100:
`100 + 10%` is `100.10`; use `100 * (1 + 10%)` for a 10% increase. No implicit
multiplication, functions, exponent notation, variables or executable code.
Lengths accept decimal, fractional and mixed-fraction quantities with an explicit
source unit: `in`, `ft`, `mm`, `cm`, `m`. All directions work. Display defaults to
two decimals; calculations and stored values retain f64 precision.

## Terminal dashboard keys

| Key | Action |
| --- | --- |
| Tab / Shift-Tab, 1–5 | Focus: deadline, tasks, calendar, calculator, length |
| F2 / Esc | Expand / return to tiles |
| Enter, i, a | Input a calculation/length or add an item |
| e | Edit selected task, deadline or calendar note |
| ↑ / ↓, j / k | Select items; scroll calculator history |
| r in calculator | Reuse selected result without rounding |
| Space in tasks | Toggle completion |
| J / K in tasks | Move task down / up |
| f in deadlines | Feature selected deadline |
| d / Delete | Delete selected task, deadline or calendar note |
| ← / → in calendar | Previous / next month |
| t in calendar | Return to current month |
| ? | Keyboard help |
| q / Ctrl-C | Save and quit |

Input appears inside the focused panel. Enter advances to the next field and
submits the last field; Tab changes fields; Ctrl-U clears a field; Backspace
removes a character; Esc cancels. Editing uses append/backspace, not a full text
editor. A deadline needs a timezone-qualified RFC3339 timestamp. Local dates and
deadline times display in the computer's timezone. Calendar `*` marks deadlines,
`+` marks dated notes, and today is highlighted. Notes below the month can be
selected and edited independently of the displayed month.

## Portable state

Default: `$XDG_DATA_HOME/jig/jig.json`, or `~/.local/share/jig/jig.json`.
`--state PATH` overrides it for every command. Copy that one file to another
computer and open it with `--state`; no migration service or database is needed.
State includes preferences, complete calculator history, task order, deadlines,
calendar notes and featured deadline ID. New terminal installations start empty;
the converter initially displays 12 inches as an instrument example. Browser demo
samples are generated in code and kept separate from saved personal state.

JSON is deliberately simpler than Markdown front matter: one established serde
parser, an explicit format version, and no ambiguity between prose and structured
data. It is human-readable and editable while Jig is stopped. No personal state
is committed; the public preview contains only synthetic samples. A terminal or
CLI save on each successful mutation uses a same-directory temporary file, file sync, atomic rename and directory sync on Linux. Invalid or
unsupported state produces an error and is never silently reset. Files created
on Unix use mode 0600.

Only one Jig process can open a given file at once. After a crash, check that no
Jig process is running, then remove the lock formed by appending `.lock` to the
entire state filename, such as `jig.json.lock` or `portable.state.lock`. A leftover
temporary file similarly appends `.tmp`, such as `portable.state.tmp`. It blocks
saves rather than overwriting possibly recoverable data: inspect it, copy it
aside or recover it, then remove it. Keep your own backups; there is no automatic
backup retention. A save failure rolls a TUI mutation back
and displays an error. Quit retries the save. Preferences are also saved on quit.

## Development

Single crate: `src/lib.rs` exposes portable logic; `src/main.rs`, `storage.rs`
and `tui.rs` adapt it for Linux files, commands and the terminal. `src/web.rs`
implements the egui browser dashboard, and `src/browser_storage.rs` plus
`web/bridge.js` adapt browser-local storage and file import/export. Terminal and
web dependencies are optional and target-specific. No filesystem, DOM, terminal,
CLI or network API is used by the core modules. Chrono supplies calendar
and timezone arithmetic; serde supplies portable serialization. Arithmetic is a
small bounded parser for the deliberately limited grammar.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/terminal_smoke.py
cargo clippy --locked --target wasm32-unknown-unknown --no-default-features --features web --bin jig-web -- -D warnings
trunk build --release --public-url /Jig/
```

Tests cover arithmetic errors and precedence, percentages, precise result reuse,
conversion round trips and fractions, checklist changes, deadlines/timezones,
leap years, labelled synthetic samples, state serialization, safe saves, locking,
CLI restart persistence, rejected intermediate arithmetic overflow, and imported
history containing terminal controls.
GitHub Actions runs the same checks, a Linux PTY smoke check, and a release build.
The PTY check verifies idle countdown redraws, keyboard input, resizing, restart
persistence and terminal restoration. CI publishes the Linux binary as an artifact.

## Limitations and next steps

The native dashboard is keyboard-only. The browser supports mouse and keyboard,
but the egui canvas currently has limited screen-reader accessibility. Browser
background tabs may throttle the otherwise one-second countdown refresh. Browser
storage can be cleared or unavailable; JSON export is the portable backup.
Browser state is capped at 2 MiB; CLI history is intentionally unbounded.

Arithmetic uses f64 rather than exact decimal/rational values. There is no undo,
recurring deadline, timezone database selection, external calendar sync,
scientific calculator or automatic backup retention. The native terminal editor
uses append/backspace editing. No native graphical application is included.
The preview bundles monospace fonts and makes no font or application-data network
requests. It has no service worker: reopening it offline depends on the browser's
asset cache; use the native binary for dependable offline launch.

The layout follows the supplied concept with five instruments; weather,
priorities and additional measurement modes are outside this milestone.
Next, validate the browser preview on real devices, improve canvas accessibility
and narrow layouts, and address concrete feedback before adding tools. The
[Actions page](https://github.com/biraz-dev/Jig/actions) shows build progress;
[preview notes](docs/preview.md) describe Pages deployment and follow-up work.
