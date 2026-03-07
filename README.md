# agent_top

`agent_top` is a Rust-based Codex session monitor with:

- a shared runner core in Rust
- a terminal UI for local monitoring
- a Tauri desktop shell for multi-run workflows

## Current Features

- Run `codex exec --json` and stream live events
- Parse Codex JSONL into normalized session events
- Monitor sessions in a Rust TUI
- Launch desktop runs with prompt, workspace, and Codex settings
- Run multiple desktop sessions in parallel
- Pick a workspace from a folder dialog in the desktop app
- Use compact session cards with expandable event feeds

## Project Layout

- [crates/agent_top_core](/C:/Users/joshs/Projects/agent_top/crates/agent_top_core)  
  Shared Rust core for event parsing, session summaries, and Codex process spawning.

- [src/main.rs](/C:/Users/joshs/Projects/agent_top/src/main.rs)  
  Terminal app built on `ratatui` and `crossterm`.

- [desktop](/C:/Users/joshs/Projects/agent_top/desktop)  
  Tauri desktop app with a TypeScript frontend and Rust backend bridge.

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
- inspect each run in its own session card
- expand a card for detailed event history

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
cargo check
```

Desktop frontend:

```powershell
cd desktop
npm run build
```

Desktop Rust backend:

```powershell
cd desktop
cargo check --manifest-path src-tauri\Cargo.toml
```

## Next Steps

- persist session history and settings
- add cancel/stop per session
- improve file activity and richer analytics
- add automated tests around runner parsing and desktop session state
