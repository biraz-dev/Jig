# Subsequent milestone: local browser preview

First validate the CLI and TUI on Linux, including keyboard use, resize behavior
and state recovery. Then add a minimal egui/eframe dashboard that calls the
existing calculator, units, model and calendar modules. Keep the project a
single crate unless separating native and browser dependencies becomes necessary.
Make clap, Ratatui, Crossterm and filesystem storage native-only through target
configuration or features; use Chrono's WASM clock support and a browser-local
time adapter for calendar display.

Use Trunk and the `wasm32-unknown-unknown` target. Render the same five useful
instruments with one expanded tool at a time. Browser-local storage replaces
filesystem writes, with explicit JSON import/export for user ownership. Browser
storage is not a backup; expose download and deletion controls. No backend,
account, analytics or remote data processing.

Ship only a clearly labelled, synthetic sample dataset, assembled in code. Never
bundle a developer's jig.json, home directory, temporary files or local uploads.
Keep sample state separate from a user's imported state; imports stay in the
browser and are never sent to GitHub.

Add a dedicated GitHub Actions Pages workflow after the browser target exists:
checkout, install stable Rust plus WASM target and pinned Trunk, test the shared
core, build `trunk build --release --public-url /Jig/`, upload **only** `dist/`
with `actions/upload-pages-artifact`, and deploy using `actions/deploy-pages`.
Grant `pages: write` and `id-token: write` only to the deploy job and use the
`github-pages` environment. Enable Actions as the Pages source in repository
settings. Deploy from main; pull requests only build and test. Do not add a
nonfunctional deployment workflow before its build target exists.

The public page should state “Development preview — sample data”, link the
source and current build status, and disclose that browser storage is local to
that browser. Check that the published artifact contains only intended static
assets. This milestone provides the plan; it does not claim a live Pages URL.
