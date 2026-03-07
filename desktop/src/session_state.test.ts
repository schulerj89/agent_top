import { describe, expect, it } from "vitest";

import {
  adjacentSessionId,
  applyAgentEvent,
  attachSessionEvents,
  createSessionState,
  filterSessionEvents,
  filterSessions,
  mergeSessionSummary,
  pickInitialSessionId,
  sortSessions,
  type SessionListItem,
} from "./session_state";

function summary(overrides: Partial<SessionListItem> = {}): SessionListItem {
  return {
    session_id: "run-1",
    title: "Fix tests",
    prompt: "Fix the failing tests",
    workspace: "c:/repo",
    lifecycle: "running",
    status: "Running",
    updated_at: "100",
    last_event_at: "100",
    last_message: "working",
    total_events: 1,
    command_count: 0,
    warning_count: 0,
    error_count: 0,
    settings: { model: "", sandbox: "workspace-write", approval: "never" },
    ...overrides,
  };
}

describe("session state", () => {
  it("applies incoming events to counters and lifecycle", () => {
    const session = attachSessionEvents(createSessionState(summary()), []);

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
    expect(next.totalEvents).toBe(2);
  });

  it("filters events by text and kind", () => {
    const session = attachSessionEvents(createSessionState(summary()), [
      { timestamp: "t1", kind: "command", message: "cargo test", sequence_no: 1 },
      { timestamp: "t2", kind: "warning", message: "stderr line", sequence_no: 2 },
    ]);

    expect(filterSessionEvents(session, { query: "cargo", kind: "all" })).toHaveLength(1);
    expect(filterSessionEvents(session, { query: "", kind: "warning" })).toHaveLength(1);
  });

  it("sorts and chooses the most recent session for initial selection", () => {
    const sessions = [
      createSessionState(summary({ session_id: "run-1", updated_at: "10" })),
      createSessionState(summary({ session_id: "run-2", updated_at: "30" })),
      createSessionState(summary({ session_id: "run-3", updated_at: "20" })),
    ];

    expect(sortSessions(sessions).map((session) => session.id)).toEqual(["run-2", "run-3", "run-1"]);
    expect(pickInitialSessionId(sessions)).toBe("run-2");
    expect(adjacentSessionId(sessions, "run-2", "next")).toBe("run-3");
    expect(adjacentSessionId(sessions, "run-2", "previous")).toBe("run-1");
  });

  it("filters sessions for the left nav", () => {
    const sessions = [
      createSessionState(summary({ session_id: "run-1", title: "Fix tests" })),
      createSessionState(summary({ session_id: "run-2", title: "Review docs", workspace: "c:/docs" })),
    ];

    expect(filterSessions(sessions, "docs")).toHaveLength(1);
    expect(filterSessions(sessions, "review")).toHaveLength(1);
  });

  it("merges refreshed sidebar summaries into existing state", () => {
    const session = createSessionState(summary());
    const next = mergeSessionSummary(
      session,
      summary({
        lifecycle: "completed",
        status: "Completed",
        total_events: 5,
        command_count: 2,
        last_message: "done",
        updated_at: "200",
      }),
    );

    expect(next.lifecycle).toBe("completed");
    expect(next.totalEvents).toBe(5);
    expect(next.latestMessage).toBe("done");
    expect(next.updatedAt).toBe("200");
  });

  it("keeps workspace and settings on session state", () => {
    const session = createSessionState(
      summary({
        workspace: "c:/workspace-a",
        settings: { model: "gpt-5", sandbox: "danger-full-access", approval: "never" },
      }),
    );

    const next = mergeSessionSummary(
      session,
      summary({
        workspace: "c:/workspace-b",
        settings: { model: "o4", sandbox: "workspace-write", approval: "on-request" },
      }),
    );

    expect(session.workspace).toBe("c:/workspace-a");
    expect(session.settings.model).toBe("gpt-5");
    expect(next.workspace).toBe("c:/workspace-b");
    expect(next.settings.sandbox).toBe("workspace-write");
    expect(next.settings.approval).toBe("on-request");
  });
});
