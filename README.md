# agent_top

`agent_top` is a terminal-first tracker for coding agent sessions.

This repository is being built in phases:

## Phase 1: Easy

Current scope:

- Load a plain-text event log
- Track session status, command counts, files touched, and recent events
- Render a simple terminal dashboard

Example use:

```powershell
cargo run -- sample\session.log
```

## Phase 2: Medium

Planned scope:

- Live follow mode for an active log file
- Better terminal layout and color
- Per-command durations and error summaries
- Filtered views for commands, files, and warnings

## Phase 3: Ambitious

Planned scope:

- Local daemon that captures agent activity
- Structured event protocol
- Web or desktop companion UI
- Session history and analytics

## Event format

Each line in the input file uses this format:

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

See [sample/session.log](/C:/Users/joshs/Projects/agent_top/sample/session.log) for a working example.
