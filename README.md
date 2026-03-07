# agent_top

`agent_top` is a Rust-based Codex session monitor with:

- a shared runner core in Rust
- a terminal UI for local monitoring
- a Tauri desktop shell for persistent multi-session workflows

## Current Features

- Run `codex exec --json` and stream live events
- Parse Codex JSONL into normalized session events
- Monitor sessions in a Rust TUI
- Launch desktop runs with prompt, workspace, and Codex settings
- Persist desktop session metadata and event history in SQLite
- Run multiple desktop sessions in parallel
- Browse desktop sessions from a left-side session nav
- Load one selected session into a dedicated detail pane
- Cancel and retry desktop runs
- Pick a workspace from a folder dialog in the desktop app
- Filter session lists and selected-session events

## Project Layout

- [crates/agent_top_core](/C:/Users/joshs/Projects/agent_top/crates/agent_top_core)  
  Shared Rust core for event parsing, session summaries, and Codex process spawning.

- [src/main.rs](/C:/Users/joshs/Projects/agent_top/src/main.rs)  
  Terminal app built on `ratatui` and `crossterm`.

- [desktop](/C:/Users/joshs/Projects/agent_top/desktop)  
  Tauri desktop app with a TypeScript frontend and Rust backend bridge.

- [desktop/src-tauri/src/storage.rs](/C:/Users/joshs/Projects/agent_top/desktop/src-tauri/src/storage.rs)  
  SQLite-backed storage layer for desktop sessions and event history.

## Terminal App

Run the TUI:

```powershell
cargo run --
```

Useful commands:

```powershell
cargo run -- replay sample\session.log
```

```powershell
cargo run -- run "Reply with the single word ready"
```

Inside the TUI:

- `n` starts a new run
- `s` opens settings
- `q` quits from the home screen

## Desktop App

Run the desktop shell:

```powershell
cd desktop
npm install
npm run tauri dev
```

Desktop workflow:

- choose a workspace folder
- enter a prompt or use `/status`
- launch multiple runs in parallel
- browse sessions from the left nav
- inspect the selected session in the detail pane
- filter selected-session events
- cancel or retry from the detail view

Desktop persistence:

- session metadata and events are stored in a local SQLite database
- the desktop app restores recent sessions on startup

## Codex Settings

Current run settings exposed in both app flows:

- `model`
- `sandbox`
- `approval`

## Event Format

Replay mode accepts plain-text logs in this format:

```text
timestamp|kind|message
```

Supported kinds:

- `status`
- `command`
- `file`
- `warning`
- `error`
- `note`

Sample log:

- [sample/session.log](/C:/Users/joshs/Projects/agent_top/sample/session.log)

## Build Checks

Root workspace:

```powershell
cargo test --workspace
```

Desktop frontend:

```powershell
cd desktop
npm install
npm test
npm run build
```

Desktop Rust backend:

```powershell
cd desktop
cargo check --manifest-path src-tauri\Cargo.toml
```

## Next Steps

- add richer selected-session analytics in the detail pane
- improve session-to-session keyboard navigation and bulk session actions
- clean up the remaining JSON-history compatibility paths
- expand automated coverage around desktop integration flows
