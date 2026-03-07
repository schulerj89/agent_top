import { describe, expect, it } from "vitest";

import { applyAgentEvent, createSessionState, filterSessionEvents } from "./session_state";

describe("session state", () => {
  it("applies incoming events to counters and lifecycle", () => {
    const session = createSessionState({
      session_id: "run-1",
      prompt: "/status",
      workspace: "c:/repo",
      lifecycle: "running",
      status: "Running",
      started_at: "1",
      updated_at: "1",
      settings: { model: "", sandbox: "workspace-write", approval: "never" },
      summary: {
        source: "live",
        current_status: "Running",
        commands: 0,
        warnings: 0,
        errors: 0,
        files_touched: [],
        recent_events: [],
        total_events: 0,
        all_events: [],
        analytics: { command_runs: [], exit_status_counts: {}, file_groups: {} },
      },
    });

    const next = applyAgentEvent(session, {
      session_id: "run-1",
      timestamp: "command",
      kind: "command",
      message: "completed",
      finished: true,
      lifecycle: "completed",
    });

    expect(next.commands).toBe(1);
    expect(next.running).toBe(false);
    expect(next.status).toBe("Completed");
  });

  it("filters events by text and kind", () => {
    const session = createSessionState({
      session_id: "run-1",
      prompt: "prompt",
      workspace: "c:/repo",
      lifecycle: "completed",
      status: "Completed",
      started_at: "1",
      updated_at: "1",
      settings: { model: "", sandbox: "workspace-write", approval: "never" },
      summary: {
        source: "live",
        current_status: "Completed",
        commands: 1,
        warnings: 1,
        errors: 0,
        files_touched: [],
        recent_events: [],
        total_events: 2,
        all_events: [
          { timestamp: "t1", kind: "command", message: "cargo test" },
          { timestamp: "t2", kind: "warning", message: "stderr line" },
        ],
        analytics: { command_runs: [], exit_status_counts: {}, file_groups: {} },
      },
    });

    expect(filterSessionEvents(session, { query: "cargo", kind: "all" })).toHaveLength(1);
    expect(filterSessionEvents(session, { query: "", kind: "warning" })).toHaveLength(1);
  });
});
