# Desktop Refactor And Session Redesign Plan

## Goals

- Reduce coupling in the desktop app so new UI work does not keep accumulating in two oversized entry files.
- Preserve current behavior while restructuring code.
- Redesign the session experience to feel closer to the attached Codex screenshot:
  - left rail as the primary navigation surface
  - sessions/threads as first-class objects
  - calmer center stage with stronger focus on the selected thread
  - cleaner progression from "new thread" to "active run" to "history"
- Keep shipping through checkpoints with tests and commits.

## Current Structural Problems

- `desktop/src/main.ts` mixes template creation, DOM lookups, state, rendering, Tauri API access, event subscriptions, and UI action wiring.
- `desktop/src-tauri/src/lib.rs` mixes command handlers, run orchestration, persistence updates, lifecycle derivation, and startup reconciliation.
- Session UI is functionally useful but visually dense and shaped more like an admin tool than a thread-first workspace.
- Resume logic and lifecycle logic are correct enough, but they are embedded in high-level entrypoints instead of being isolated in domain services.

## Refactor Checkpoint 1

### Backend

- Split `desktop/src-tauri/src/lib.rs` into modules:
  - `app_state.rs`: shared app state and common request/response payloads
  - `commands.rs`: Tauri command handlers and frontend-facing DTO mapping
  - `runtime.rs`: run lifecycle, event forwarding, resume policy, startup reconciliation
  - keep `storage.rs` as persistence
- Do not change command names or payload shapes in this checkpoint.
- Preserve SQLite schema and stored data compatibility.

### Frontend

- Split `desktop/src/main.ts` into:
  - `app_shell.ts`: HTML shell markup
  - `dom.ts`: typed DOM lookup
  - `tauri_api.ts`: runtime detection and invoke/listen wrapper
  - `app.ts`: app controller/state orchestration
- Keep `session_state.ts` as the pure state helper layer.
- Preserve current behavior and keyboard shortcuts.

### Tests For Checkpoint 1

- Rust:
  - `cargo test --workspace`
  - `cargo check --manifest-path desktop/src-tauri/Cargo.toml`
- Frontend:
  - `npm test`
  - `npm run build`

## Session Redesign Backlog

### Phase 1: Information Architecture

- Promote the left rail from a filtered list into a true thread navigator.
- Separate rail sections:
  - new thread action
  - automations or saved workflows placeholder
  - threads/sessions list
  - lower utility/settings area
- Narrow what each session row shows:
  - title
  - relative time
  - subtle running indicator
- Move most session detail metadata out of the rail and into the main workspace.

### Phase 2: Main Workspace Layout

- Make the selected thread the center of gravity.
- Replace the current "hero plus two panels" landing layout with a calmer thread workspace:
  - empty/new-thread state in the center
  - selected thread timeline when a session exists
  - composer anchored low in the viewport
- Collapse global stats into either:
  - a small top utility row, or
  - a secondary inspector panel

### Phase 3: Session Detail Model

- Distinguish between:
  - thread metadata
  - run attempts within a thread
  - event timeline for the selected run
- Likely data shape extension:
  - thread id
  - run id
  - run attempt number
  - run lifecycle summary
- This probably requires a schema follow-up after the refactor checkpoint.

### Phase 4: Interaction Design

- Support new-thread flow without forcing a prior session selection.
- Make continue/retry semantics explicit in the UI:
  - continue current thread
  - branch/retry as a new run
- Add active session focus states and better keyboard navigation.

### Phase 5: Visual Direction

- Shift toward the screenshot's stronger hierarchy:
  - darker, quieter canvas
  - persistent left navigation
  - centered thread focus
  - lower-contrast chrome around utility actions
- Keep the existing orange accent only if it still fits after the layout rewrite.

## Delivery Strategy

1. Land behavior-preserving structural cleanup first.
2. Re-run tests and commit.
3. Start UI redesign on top of cleaner modules, not on top of oversized entry files.
4. Ship the session rail/layout redesign in smaller commits:
   - rail restructuring
   - empty state and workspace shell
   - composer relocation
   - session row redesign
   - thread/run detail model improvements

## Stop Conditions

- If the same blocker is hit more than three times in the same area, stop and reassess before forcing a brittle solution.
- Do not mix schema redesign with the first cleanup checkpoint unless the current structure makes it unavoidable.
