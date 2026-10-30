# Jig

A tool for creative Small Giants. A small, local artist's workbench in Rust:
calculator, length conversion, tasks, deadlines and calendar. No accounts,
telemetry, network calls, database or background service.

![Jig terminal dashboard — sample data](docs/sample-dashboard.svg)

This is a rendering of the actual Ratatui dashboard using clearly labelled sample
data, not a browser application.

## Build and run on Linux

Install the stable Rust toolchain with rustup, then:

```sh
git clone --branch jig-initial-milestone https://github.com/biraz-dev/Jig.git
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

## Dashboard keys

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
calendar notes and featured deadline ID. New installations start empty; the
converter initially displays 12 inches as an instrument example.

JSON is deliberately simpler than Markdown front matter: one established serde
parser, an explicit format version, and no ambiguity between prose and structured
data. It is human-readable and editable while Jig is stopped. No sample or
personal state is committed. Save on each successful mutation uses a same-directory
temporary file, file sync, atomic rename and directory sync on Linux. Invalid or
unsupported state produces an error and is never silently reset. Files created
on Unix use mode 0600.

Only one Jig process can open a given file at once. After a crash, check that no
Jig process is running, then remove the corresponding `.json.lock`. A leftover
`.json.tmp` blocks saves rather than overwriting possibly recoverable data:
inspect it, copy it aside or recover it, then remove it. Keep your own backups;
there is no automatic backup retention. A save failure rolls a TUI mutation back
and displays an error. Quit retries the save. Preferences are also saved on quit.

## Development

Single crate: `src/lib.rs` exposes portable logic; `src/main.rs`, `storage.rs`
and `tui.rs` adapt it for Linux files, commands and the terminal. No filesystem,
terminal, CLI or network API is used by the core modules. Chrono supplies calendar
and timezone arithmetic; serde supplies portable serialization. Arithmetic is a
small bounded parser for the deliberately limited grammar.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/terminal_smoke.py
```

Tests cover arithmetic errors and precedence, percentages, precise result reuse,
conversion round trips and fractions, checklist changes, deadlines/timezones,
leap years, state serialization, safe saves, locking and CLI restart persistence.
GitHub Actions runs the same checks, a Linux PTY smoke check, and a release build.
The PTY check verifies idle countdown redraws, keyboard input, resizing, restart
persistence and terminal restoration. CI publishes the Linux binary as an artifact.

## Limitations and next milestone

This is the initial terminal/CLI implementation. Keyboard support only; no mouse,
undo, recurring deadlines, timezone database selection, external calendar sync,
scientific calculator, rational/decimal exact arithmetic or automatic backups.
History is intentionally unbounded. The terminal layout and palette follow the supplied concept image, with the
five requested instruments and no weather, priorities or extra measurement modes.
Terminal layout and input behavior still need testing on real terminal emulators.

The browser preview is deliberately deferred. See [the preview plan](docs/preview.md).
The [Actions page](https://github.com/biraz-dev/Jig/actions) shows public build
progress. No graphical interface or browser application ships in this milestone.
