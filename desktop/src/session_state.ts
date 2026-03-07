export type Kind = "status" | "command" | "file" | "warning" | "error" | "note";
export type Lifecycle =
  | "launching"
  | "running"
  | "cancelling"
  | "cancelled"
  | "completed"
  | "failed";

export type AgentEvent = {
  session_id: string;
  timestamp: string;
  kind: Kind;
  message: string;
  finished: boolean;
  lifecycle: Lifecycle;
};

export type SessionEvent = {
  timestamp: string;
  kind: Kind;
  message: string;
};

export type SessionSummary = {
  source: string;
  current_status: string | null;
  commands: number;
  warnings: number;
  errors: number;
  files_touched: string[];
  recent_events: SessionEvent[];
  total_events: number;
  all_events: SessionEvent[];
  analytics: {
    command_runs: Array<{
      command: string;
      exit_code: number | null;
      duration_ms: number | null;
    }>;
    exit_status_counts: Record<string, number>;
    file_groups: Record<string, number>;
  };
};

export type SessionRecord = {
  session_id: string;
  prompt: string;
  workspace: string;
  lifecycle: Lifecycle;
  status: string;
  summary: SessionSummary;
  settings: {
    model: string;
    sandbox: string;
    approval: string;
  };
  started_at: string;
  updated_at: string;
};

export type SessionState = {
  id: string;
  prompt: string;
  workspace: string;
  status: string;
  lifecycle: Lifecycle;
  running: boolean;
  events: SessionEvent[];
  commands: number;
  warnings: number;
  latestMessage: string;
};

export type SessionFilter = {
  query: string;
  kind: Kind | "all";
};

export function createSessionState(record: SessionRecord): SessionState {
  const events = [...record.summary.all_events];
  const latest = events.at(-1)?.message ?? "waiting for first event";

  return {
    id: record.session_id,
    prompt: record.prompt,
    workspace: record.workspace,
    status: record.status,
    lifecycle: record.lifecycle,
    running: record.lifecycle === "launching" || record.lifecycle === "running" || record.lifecycle === "cancelling",
    events,
    commands: record.summary.commands,
    warnings: record.summary.warnings + record.summary.errors,
    latestMessage: latest,
  };
}

export function applyAgentEvent(session: SessionState, event: AgentEvent): SessionState {
  const events = [...session.events, { timestamp: event.timestamp, kind: event.kind, message: event.message }];
  return {
    ...session,
    status: event.finished ? titleFromLifecycle(event.lifecycle) : event.kind === "status" ? event.message : "Running",
    lifecycle: event.lifecycle,
    running: !event.finished && (event.lifecycle === "launching" || event.lifecycle === "running" || event.lifecycle === "cancelling"),
    events,
    commands: session.commands + (event.kind === "command" ? 1 : 0),
    warnings: session.warnings + (event.kind === "warning" || event.kind === "error" ? 1 : 0),
    latestMessage: event.message,
  };
}

export function filterSessionEvents(session: SessionState, filter: SessionFilter): SessionEvent[] {
  const query = filter.query.trim().toLowerCase();
  return session.events.filter((event) => {
    if (filter.kind !== "all" && event.kind !== filter.kind) {
      return false;
    }

    if (!query) {
      return true;
    }

    return (
      event.message.toLowerCase().includes(query) ||
      event.timestamp.toLowerCase().includes(query) ||
      event.kind.toLowerCase().includes(query)
    );
  });
}

export function titleFromLifecycle(lifecycle: Lifecycle): string {
  switch (lifecycle) {
    case "launching":
      return "Launching";
    case "running":
      return "Running";
    case "cancelling":
      return "Cancelling";
    case "cancelled":
      return "Cancelled";
    case "completed":
      return "Completed";
    case "failed":
      return "Failed";
  }
}
