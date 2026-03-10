# agent_top

`agent_top` is a Codex runner and monitor built around a shared Rust core.

It currently ships two interfaces:

- a Rust terminal UI for local monitoring and replay
- a Tauri desktop app for persistent thread-based workflows

## What It Does

`agent_top` wraps `codex exec --json`, parses the event stream, and presents it in a UI that can:

- start new Codex runs
- continue an existing thread with a new run attempt
- retry a thread by launching another run attempt
- persist thread metadata and event history in SQLite
- replay saved event logs in the terminal UI

## Architecture

### Shared Core

`crates/agent_top_core` contains the common runtime logic:

- spawning Codex processes
- parsing JSON events into normalized internal events
- tracking summaries, file touches, and command activity
- cancellation support
- resume eligibility checks

### Terminal UI

`src/main.rs` provides a `ratatui`/`crossterm` interface for:

- starting runs
- watching live status and event streams
- replaying saved logs

### Desktop App

`desktop` contains the Tauri app:

- `desktop/src` is the TypeScript frontend
- `desktop/src-tauri/src` is the Rust backend
- `desktop/src-tauri/src/storage.rs` manages SQLite persistence

The desktop app now uses a thread/run-attempt model:

- a thread represents the long-lived conversation or workspace context
- each launch, continue, or retry creates a new run attempt inside that thread
- the UI shows threads in the left rail and resolves the selected timeline from the active or latest run attempt

## Terminal Usage

Run the TUI:

```powershell
cargo run --
```

Replay a saved log:

```powershell
cargo run -- replay sample\session.log
```

Start a run directly:

```powershell
cargo run -- run "Reply with the single word ready"
```

Inside the TUI:

- `n` starts a new run
- `s` opens settings
- `q` quits from the home screen

## Desktop Usage

Start the desktop app in development:

```powershell
cd desktop
npm install
npm run tauri dev
```

Typical workflow:

1. Choose a workspace folder.
2. Enter a prompt.
3. Pick Codex settings such as model, sandbox, and approval mode.
4. Start a run, continue a thread, or retry a thread.
5. Inspect thread history and the latest run activity in the main pane.

Notes:

- the selected workspace is shown as the current workspace for the next run
- the thread also keeps its historical workspace so you can tell current selection from prior context
- recent threads and run history are restored from SQLite on startup

## Codex Settings

The current desktop and terminal flows expose:

- `model`
- `sandbox`
- `approval`
- bypass behavior where supported by the desktop flow

## Event Replay Format

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

- `sample/session.log`

## Build And Test

Run the Rust workspace tests:

```powershell
cargo test --workspace
```

Check the desktop Rust backend:

```powershell
cd desktop
cargo check --manifest-path src-tauri\Cargo.toml
```

Run the desktop frontend checks:

```powershell
cd desktop
npm install
npm test
npm run build
```

Build desktop release artifacts:

```powershell
cd desktop
npm run tauri build
```

## Current Focus

The current desktop work is aimed at:

- improving the thread-oriented UX
- making run-attempt history clearer inside each thread
- strengthening typed lifecycle/state handling across the backend and frontend
- expanding tests around thread continuation and retry flows
