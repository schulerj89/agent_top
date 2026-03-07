# TODO

## Active Phases

- Phase 5: cleanup
  - completed: removed remaining JSON-history compatibility paths from the shared core
  - completed: removed stale SQLite startup compatibility glue
  - completed: reran Rust and desktop validation before merge

- Phase 6: sidebar shell redesign
  - completed: moved the session nav into a true left rail anchored to the window edge
  - completed: added a visible separator between the left rail and the main content pane
  - completed: supported collapsed and expanded sidebar states
  - completed: kept session cycling available from both sidebar clicks and keyboard controls
  - completed: reorganized the main content so the composer and selected-session detail pane feel like the primary workspace, not nested cards
  - completed: validated desktop layout through frontend tests and build checks
