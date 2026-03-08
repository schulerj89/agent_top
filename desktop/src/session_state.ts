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

export type SessionSettings = {
  model: string;
  sandbox: string;
  approval: string;
};

export type SessionListItem = {
  session_id: string;
  title: string;
  prompt: string;
  workspace: string;
  codex_session_id?: string | null;
  lifecycle: Lifecycle;
  status: string;
  updated_at: string;
  last_event_at: string | null;
  last_message: string | null;
  total_events: number;
  command_count: number;
  warning_count: number;
  error_count: number;
  settings: SessionSettings;
};

export type SessionEvent = {
  id?: number;
  session_id?: string;
  timestamp: string;
  kind: Kind;
  message: string;
  payload_json?: string | null;
  sequence_no?: number;
};

export type SessionState = {
  id: string;
  title: string;
  prompt: string;
  workspace: string;
  codexSessionId: string | null;
  settings: SessionSettings;
  status: string;
  lifecycle: Lifecycle;
  running: boolean;
  events: SessionEvent[];
  eventsLoaded: boolean;
  totalEvents: number;
  commands: number;
  warnings: number;
  latestMessage: string;
  updatedAt: string;
};

export type SessionFilter = {
  query: string;
  kind: Kind | "all";
};

export function createSessionState(record: SessionListItem): SessionState {
  return {
    id: record.session_id,
    title: record.title,
    prompt: record.prompt,
    workspace: record.workspace,
    codexSessionId: record.codex_session_id ?? null,
    settings: record.settings,
    status: record.status,
    lifecycle: record.lifecycle,
    running: isActiveLifecycle(record.lifecycle),
    events: [],
    eventsLoaded: false,
    totalEvents: record.total_events,
    commands: record.command_count,
    warnings: record.warning_count + record.error_count,
    latestMessage: record.last_message ?? "waiting for first event",
    updatedAt: record.updated_at,
  };
}

export function mergeSessionSummary(session: SessionState, summary: SessionListItem): SessionState {
  return {
    ...session,
    title: summary.title,
    prompt: summary.prompt,
    workspace: summary.workspace,
    codexSessionId: summary.codex_session_id ?? session.codexSessionId,
    settings: summary.settings,
    status: summary.status,
    lifecycle: summary.lifecycle,
    running: isActiveLifecycle(summary.lifecycle),
    totalEvents: summary.total_events,
    commands: summary.command_count,
    warnings: summary.warning_count + summary.error_count,
    latestMessage: summary.last_message ?? session.latestMessage,
    updatedAt: summary.updated_at,
  };
}

export function attachSessionEvents(session: SessionState, events: SessionEvent[]): SessionState {
  return {
    ...session,
    events: [...events].sort((left, right) => (left.sequence_no ?? 0) - (right.sequence_no ?? 0)),
    eventsLoaded: true,
    totalEvents: Math.max(session.totalEvents, events.length),
    latestMessage: events.at(-1)?.message ?? session.latestMessage,
  };
}

export function applyAgentEvent(session: SessionState, event: AgentEvent): SessionState {
  const nextEvent: SessionEvent = {
    session_id: event.session_id,
    timestamp: event.timestamp,
    kind: event.kind,
    message: event.message,
  };

  return {
    ...session,
    status: event.finished ? titleFromLifecycle(event.lifecycle) : event.kind === "status" ? event.message : "Running",
    lifecycle: event.lifecycle,
    running: isActiveLifecycle(event.lifecycle) && !event.finished,
    events: session.eventsLoaded ? [...session.events, nextEvent] : session.events,
    totalEvents: session.totalEvents + 1,
    commands: session.commands + (event.kind === "command" ? 1 : 0),
    warnings: session.warnings + (event.kind === "warning" || event.kind === "error" ? 1 : 0),
    latestMessage: event.message,
    updatedAt: event.timestamp,
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

export function filterSessions(sessions: SessionState[], query: string): SessionState[] {
  const normalized = query.trim().toLowerCase();
  return sessions.filter((session) => {
    if (!normalized) {
      return true;
    }

    return (
      session.title.toLowerCase().includes(normalized) ||
      session.prompt.toLowerCase().includes(normalized) ||
      session.workspace.toLowerCase().includes(normalized) ||
      session.latestMessage.toLowerCase().includes(normalized)
    );
  });
}

export function sortSessions(sessions: SessionState[]): SessionState[] {
  return [...sessions].sort((left, right) => {
    const updated = Number(right.updatedAt) - Number(left.updatedAt);
    return updated !== 0 ? updated : right.id.localeCompare(left.id);
  });
}

export function pickInitialSessionId(sessions: SessionState[]): string | null {
  return sortSessions(sessions)[0]?.id ?? null;
}

export function adjacentSessionId(
  sessions: SessionState[],
  currentId: string | null,
  direction: "next" | "previous",
): string | null {
  const ordered = sortSessions(sessions);
  if (ordered.length === 0) {
    return null;
  }

  if (!currentId) {
    return ordered[0].id;
  }

  const index = ordered.findIndex((session) => session.id === currentId);
  if (index === -1) {
    return ordered[0].id;
  }

  const delta = direction === "next" ? 1 : -1;
  return ordered[(index + delta + ordered.length) % ordered.length]?.id ?? null;
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

function isActiveLifecycle(lifecycle: Lifecycle): boolean {
  return lifecycle === "launching" || lifecycle === "running" || lifecycle === "cancelling";
}
