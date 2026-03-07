import "./style.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  titleFromLifecycle,
  type AgentEvent,
  type Kind,
  type SessionEvent,
  type SessionFilter,
  type SessionListItem,
  type SessionState,
} from "./session_state";

type Settings = {
  model: string;
  sandbox: string;
  approval: string;
};

type Bootstrap = {
  workspace: string;
  settings: Settings;
  sessions: SessionListItem[];
};

type StartRunResponse = {
  session_id: string;
};

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("app root not found");
}

app.innerHTML = `
  <main class="app-shell">
    <aside id="sidebarRail" class="sidebar-rail" data-collapsed="false">
      <div class="rail-header">
        <div class="rail-brand">
          <p class="eyebrow">Desktop Monitor</p>
          <h1 class="rail-title">Agent Top</h1>
        </div>
        <button id="sidebarToggleButton" class="ghost rail-toggle" type="button" aria-label="Toggle sidebar">Collapse</button>
      </div>

      <div class="rail-search-wrap">
        <label class="field rail-search-field">
          <span>Search Sessions</span>
          <input id="navSearchInput" type="text" placeholder="Search title, prompt, workspace, latest" />
        </label>
      </div>

      <div id="sessionNav" class="session-nav"></div>

      <div class="rail-footer">
        <p>Shortcuts</p>
        <p>[ / ] cycle sessions</p>
      </div>
    </aside>

    <div class="shell-separator"></div>

    <section class="content-shell">
      <section class="hero">
        <div>
          <p class="eyebrow">Session Workspace</p>
          <h1>Agent Top</h1>
          <p class="summary">Use the left rail like a real session navigator: collapse it when you want more room, expand it when you want context, and keep the selected session as the primary workspace.</p>
        </div>
        <div class="hero-meta">
          <div class="meta-card"><span>Active Runs</span><strong id="activeRuns">0</strong></div>
          <div class="meta-card"><span>Total Runs</span><strong id="totalRuns">0</strong></div>
          <div class="meta-card"><span>Total Events</span><strong id="totalEvents">0</strong></div>
          <div class="meta-card"><span>Total Warnings</span><strong id="totalWarnings">0</strong></div>
        </div>
      </section>

      <section class="workspace-bar panel">
        <div>
          <p class="panel-kicker">Workspace</p>
          <strong id="workspaceLabel">Loading...</strong>
        </div>
        <div class="workspace-actions">
          <button id="chooseFolderButton" class="ghost">Choose Folder</button>
          <button id="addRunButton" class="primary">Start Run</button>
        </div>
      </section>

      <section id="errorBanner" class="error-banner hidden" role="alert"></section>

      <section class="content-grid">
        <section class="panel composer-panel">
          <header class="panel-header">
            <h2>Composer</h2>
            <p>Launch new runs with the same validated controls. New sessions appear in the left rail immediately.</p>
          </header>
          <label class="field">
            <span>Prompt</span>
            <textarea id="promptInput" rows="6" placeholder="Describe the task for Codex."></textarea>
          </label>
          <div class="settings-grid">
            <label class="field">
              <span>Model</span>
              <input id="modelInput" type="text" placeholder="default" />
            </label>
            <label class="field">
              <span>Sandbox</span>
              <select id="sandboxInput">
                <option value="read-only">read-only</option>
                <option value="workspace-write">workspace-write</option>
                <option value="danger-full-access">danger-full-access</option>
              </select>
            </label>
            <label class="field">
              <span>Approval</span>
              <select id="approvalInput">
                <option value="untrusted">untrusted</option>
                <option value="on-request">on-request</option>
                <option value="never">never</option>
              </select>
            </label>
          </div>
          <p id="composerMessage" class="run-message">Ready.</p>
        </section>

        <section class="panel detail-panel">
          <header class="detail-header">
            <div>
              <p class="panel-kicker">Selected Session</p>
              <h2 id="detailTitle">No session selected</h2>
              <p id="detailSubtitle" class="detail-subtitle">Choose a session from the left rail.</p>
            </div>
            <div class="detail-actions">
              <button id="previousSessionButton" class="ghost">Previous</button>
              <button id="nextSessionButton" class="ghost">Next</button>
              <button id="cancelRunButton" class="ghost">Cancel</button>
              <button id="retryRunButton" class="ghost">Retry</button>
            </div>
          </header>

          <div class="detail-meta">
            <div class="meta-pill"><span>Status</span><strong id="detailStatus">-</strong></div>
            <div class="meta-pill"><span>Events</span><strong id="detailEvents">0</strong></div>
            <div class="meta-pill"><span>Commands</span><strong id="detailCommands">0</strong></div>
            <div class="meta-pill"><span>Warnings</span><strong id="detailWarnings">0</strong></div>
          </div>

          <div class="detail-copy">
            <p id="detailWorkspace" class="detail-workspace"></p>
            <p id="detailPrompt" class="detail-prompt"></p>
            <p id="detailLatest" class="detail-latest"></p>
          </div>

          <div class="session-filters">
            <input id="eventSearchInput" type="text" placeholder="Search events in the selected session" />
            <select id="kindFilter">
              <option value="all">All kinds</option>
              <option value="status">Status</option>
              <option value="command">Command</option>
              <option value="file">File</option>
              <option value="warning">Warning</option>
              <option value="error">Error</option>
              <option value="note">Note</option>
            </select>
          </div>

          <p id="detailMessage" class="run-message">Session details load on demand.</p>
          <ul id="detailEventsList" class="detail-events"></ul>
        </section>
      </section>
    </section>
  </main>
`;

const sidebarRail = document.querySelector<HTMLElement>("#sidebarRail")!;
const sidebarToggleButton = document.querySelector<HTMLButtonElement>("#sidebarToggleButton")!;
const workspaceLabel = document.querySelector<HTMLElement>("#workspaceLabel")!;
const activeRuns = document.querySelector<HTMLElement>("#activeRuns")!;
const totalRuns = document.querySelector<HTMLElement>("#totalRuns")!;
const totalEvents = document.querySelector<HTMLElement>("#totalEvents")!;
const totalWarnings = document.querySelector<HTMLElement>("#totalWarnings")!;
const promptInput = document.querySelector<HTMLTextAreaElement>("#promptInput")!;
const modelInput = document.querySelector<HTMLInputElement>("#modelInput")!;
const sandboxInput = document.querySelector<HTMLSelectElement>("#sandboxInput")!;
const approvalInput = document.querySelector<HTMLSelectElement>("#approvalInput")!;
const chooseFolderButton = document.querySelector<HTMLButtonElement>("#chooseFolderButton")!;
const addRunButton = document.querySelector<HTMLButtonElement>("#addRunButton")!;
const composerMessage = document.querySelector<HTMLElement>("#composerMessage")!;
const errorBanner = document.querySelector<HTMLElement>("#errorBanner")!;
const navSearchInput = document.querySelector<HTMLInputElement>("#navSearchInput")!;
const sessionNav = document.querySelector<HTMLDivElement>("#sessionNav")!;
const detailTitle = document.querySelector<HTMLElement>("#detailTitle")!;
const detailSubtitle = document.querySelector<HTMLElement>("#detailSubtitle")!;
const detailStatus = document.querySelector<HTMLElement>("#detailStatus")!;
const detailEvents = document.querySelector<HTMLElement>("#detailEvents")!;
const detailCommands = document.querySelector<HTMLElement>("#detailCommands")!;
const detailWarnings = document.querySelector<HTMLElement>("#detailWarnings")!;
const detailWorkspace = document.querySelector<HTMLElement>("#detailWorkspace")!;
const detailPrompt = document.querySelector<HTMLElement>("#detailPrompt")!;
const detailLatest = document.querySelector<HTMLElement>("#detailLatest")!;
const detailMessage = document.querySelector<HTMLElement>("#detailMessage")!;
const previousSessionButton = document.querySelector<HTMLButtonElement>("#previousSessionButton")!;
const nextSessionButton = document.querySelector<HTMLButtonElement>("#nextSessionButton")!;
const cancelRunButton = document.querySelector<HTMLButtonElement>("#cancelRunButton")!;
const retryRunButton = document.querySelector<HTMLButtonElement>("#retryRunButton")!;
const eventSearchInput = document.querySelector<HTMLInputElement>("#eventSearchInput")!;
const kindFilter = document.querySelector<HTMLSelectElement>("#kindFilter")!;
const detailEventsList = document.querySelector<HTMLUListElement>("#detailEventsList")!;

let currentWorkspace = "";
let loading = true;
let loadingDetail = false;
let selectedSessionId: string | null = null;
let sidebarCollapsed = false;
const sessions = new Map<string, SessionState>();
let navSearch = "";
let eventFilter: SessionFilter = { query: "", kind: "all" };

function updateHeroStats() {
  const values = [...sessions.values()];
  activeRuns.textContent = String(values.filter((session) => session.running).length);
  totalRuns.textContent = String(values.length);
  totalEvents.textContent = String(values.reduce((sum, session) => sum + session.totalEvents, 0));
  totalWarnings.textContent = String(values.reduce((sum, session) => sum + session.warnings, 0));
}

function currentSettings(): Settings {
  return {
    model: modelInput.value.trim(),
    sandbox: sandboxInput.value,
    approval: approvalInput.value,
  };
}

function setComposerMessage(message: string) {
  composerMessage.textContent = message;
}

function setError(message: string | null) {
  if (!message) {
    errorBanner.classList.add("hidden");
    errorBanner.textContent = "";
    return;
  }

  errorBanner.classList.remove("hidden");
  errorBanner.textContent = message;
}

function setLoadingState(isLoading: boolean) {
  loading = isLoading;
  chooseFolderButton.disabled = isLoading;
  addRunButton.disabled = isLoading;
}

function renderSidebarState() {
  sidebarRail.dataset.collapsed = String(sidebarCollapsed);
  sidebarToggleButton.textContent = sidebarCollapsed ? "Expand" : "Collapse";
}

function upsertSession(summary: SessionListItem) {
  const existing = sessions.get(summary.session_id);
  sessions.set(
    summary.session_id,
    existing ? mergeSessionSummary(existing, summary) : createSessionState(summary),
  );
}

function sortedVisibleSessions(): SessionState[] {
  return sortSessions(filterSessions([...sessions.values()], navSearch));
}

function ensureSelection() {
  if (selectedSessionId && sessions.has(selectedSessionId)) {
    return;
  }

  selectedSessionId = pickInitialSessionId([...sessions.values()]);
}

function compactSessionLabel(session: SessionState): string {
  const first = session.title.trim().charAt(0) || session.id.charAt(0);
  return first.toUpperCase();
}

function renderSessionNav() {
  const visible = sortedVisibleSessions();
  sessionNav.replaceChildren(
    ...(visible.length > 0
      ? visible.map((session) => {
          const button = document.createElement("button");
          button.type = "button";
          button.className = "session-nav-item";
          if (session.id === selectedSessionId) {
            button.dataset.active = "true";
          }

          if (sidebarCollapsed) {
            button.innerHTML = `
              <span class="session-nav-compact">${compactSessionLabel(session)}</span>
            `;
            button.title = `${session.title}\n${session.latestMessage}`;
          } else {
            button.innerHTML = `
              <span class="session-nav-title"></span>
              <span class="session-nav-status">${titleFromLifecycle(session.lifecycle)}</span>
              <span class="session-nav-meta"></span>
              <span class="session-nav-latest"></span>
            `;
            button.querySelector<HTMLElement>(".session-nav-title")!.textContent = session.title;
            button.querySelector<HTMLElement>(".session-nav-meta")!.textContent =
              `${session.totalEvents} events | ${session.workspace}`;
            button.querySelector<HTMLElement>(".session-nav-latest")!.textContent = session.latestMessage;
          }

          button.addEventListener("click", async () => {
            await selectSession(session.id);
          });
          return button;
        })
      : [
          Object.assign(document.createElement(sidebarCollapsed ? "span" : "p"), {
            className: "empty-sessions",
            textContent: sidebarCollapsed ? "0" : "No sessions match the current search.",
          }),
        ]),
  );
}

function renderSelectedSession() {
  const session = selectedSessionId ? sessions.get(selectedSessionId) ?? null : null;

  if (!session) {
    detailTitle.textContent = "No session selected";
    detailSubtitle.textContent = "Choose a session from the left rail.";
    detailStatus.textContent = "-";
    detailEvents.textContent = "0";
    detailCommands.textContent = "0";
    detailWarnings.textContent = "0";
    detailWorkspace.textContent = "";
    detailPrompt.textContent = "";
    detailLatest.textContent = "";
    detailMessage.textContent = "Session details load on demand.";
    detailEventsList.replaceChildren();
    previousSessionButton.disabled = true;
    nextSessionButton.disabled = true;
    cancelRunButton.disabled = true;
    retryRunButton.disabled = true;
    return;
  }

  detailTitle.textContent = session.title;
  detailSubtitle.textContent = `${session.id} | ${session.updatedAt}`;
  detailStatus.textContent = session.status;
  detailEvents.textContent = String(session.totalEvents);
  detailCommands.textContent = String(session.commands);
  detailWarnings.textContent = String(session.warnings);
  detailWorkspace.textContent = session.workspace;
  detailPrompt.textContent = session.prompt;
  detailLatest.textContent = `Latest: ${session.latestMessage}`;
  detailMessage.textContent = loadingDetail
    ? "Loading session events..."
    : session.eventsLoaded
      ? "Session events are loaded from SQLite."
      : "Select a session to load its events.";
  const ordered = sortedVisibleSessions();
  previousSessionButton.disabled = ordered.length < 2;
  nextSessionButton.disabled = ordered.length < 2;
  cancelRunButton.disabled = !session.running;
  retryRunButton.disabled = session.running;

  const visibleEvents = session.eventsLoaded ? filterSessionEvents(session, eventFilter).slice(-100).reverse() : [];
  detailEventsList.replaceChildren(
    ...(visibleEvents.length > 0
      ? visibleEvents.map((event) => {
          const item = document.createElement("li");
          item.className = `event-item kind-${event.kind}`;
          item.innerHTML = `
            <span class="event-time">${event.timestamp}</span>
            <span class="event-kind">${event.kind}</span>
            <span class="event-message"></span>
          `;
          item.querySelector<HTMLElement>(".event-message")!.textContent = event.message;
          return item;
        })
      : [Object.assign(document.createElement("li"), { className: "empty-events", textContent: session.eventsLoaded ? "No matching events." : "No events loaded yet." })]),
  );
}

function renderAll() {
  ensureSelection();
  renderSidebarState();
  renderSessionNav();
  renderSelectedSession();
  updateHeroStats();
}

async function runGuarded(action: () => Promise<void>, fallback: string) {
  try {
    setError(null);
    await action();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setError(message || fallback);
    setComposerMessage(message || fallback);
  }
}

async function selectSession(sessionId: string) {
  selectedSessionId = sessionId;
  renderAll();

  const session = sessions.get(sessionId);
  if (!session || session.eventsLoaded) {
    return;
  }

  await runGuarded(async () => {
    loadingDetail = true;
    renderSelectedSession();
    const events = await invoke<SessionEvent[]>("get_session_events", {
      request: { session_id: sessionId, limit: 250 },
    });
    const current = sessions.get(sessionId);
    if (current) {
      sessions.set(sessionId, attachSessionEvents(current, events));
    }
  }, "Unable to load session events.");

  loadingDetail = false;
  renderSelectedSession();
  renderSessionNav();
}

async function stepSelectedSession(direction: "next" | "previous") {
  const nextId = adjacentSessionId(sortedVisibleSessions(), selectedSessionId, direction);
  if (!nextId || nextId === selectedSessionId) {
    return;
  }

  await selectSession(nextId);
}

async function startRun(prompt: string) {
  const trimmedPrompt = prompt.trim();
  if (!trimmedPrompt) {
    setComposerMessage("Prompt is required.");
    return;
  }

  if (!currentWorkspace.trim()) {
    setComposerMessage("Choose a workspace first.");
    return;
  }

  await runGuarded(async () => {
    setLoadingState(true);
    setComposerMessage("Starting Codex run...");
    const response = await invoke<StartRunResponse>("start_run", {
      request: {
        prompt: trimmedPrompt,
        workspace: currentWorkspace,
        settings: currentSettings(),
      },
    });

    upsertSession({
      session_id: response.session_id,
      title: trimmedPrompt.length > 48 ? `${trimmedPrompt.slice(0, 45)}...` : trimmedPrompt,
      prompt: trimmedPrompt,
      workspace: currentWorkspace,
      lifecycle: "launching",
      status: "Launching",
      updated_at: String(Date.now()),
      last_event_at: null,
      last_message: "waiting for first event",
      total_events: 0,
      command_count: 0,
      warning_count: 0,
      error_count: 0,
      settings: currentSettings(),
    });

    selectedSessionId = response.session_id;
    renderAll();
    setComposerMessage(`Started ${response.session_id}.`);
  }, "Unable to start run.");

  setLoadingState(false);
}

async function bootstrap() {
  await runGuarded(async () => {
    setLoadingState(true);
    const payload = await invoke<Bootstrap>("bootstrap");
    currentWorkspace = payload.workspace;
    workspaceLabel.textContent = payload.workspace;
    modelInput.value = payload.settings.model;
    sandboxInput.value = payload.settings.sandbox;
    approvalInput.value = payload.settings.approval;

    sessions.clear();
    for (const summary of payload.sessions) {
      upsertSession(summary);
    }

    ensureSelection();
    renderAll();

    if (selectedSessionId) {
      await selectSession(selectedSessionId);
    }

    setComposerMessage(`Loaded ${payload.sessions.length} persisted sessions.`);
  }, "Unable to bootstrap desktop state.");

  setLoadingState(false);
}

sidebarToggleButton.addEventListener("click", () => {
  sidebarCollapsed = !sidebarCollapsed;
  renderAll();
});

chooseFolderButton.addEventListener("click", async () => {
  if (loading) {
    return;
  }

  await runGuarded(async () => {
    chooseFolderButton.disabled = true;
    const selected = await invoke<string | null>("pick_workspace");
    if (typeof selected === "string" && selected.trim()) {
      currentWorkspace = selected;
      workspaceLabel.textContent = selected;
      setComposerMessage("Workspace updated.");
    }
  }, "Unable to choose workspace.");
  chooseFolderButton.disabled = false;
});

addRunButton.addEventListener("click", async () => {
  await startRun(promptInput.value);
});

navSearchInput.addEventListener("input", () => {
  navSearch = navSearchInput.value;
  renderSessionNav();
});

eventSearchInput.addEventListener("input", () => {
  eventFilter = { ...eventFilter, query: eventSearchInput.value };
  renderSelectedSession();
});

kindFilter.addEventListener("change", () => {
  eventFilter = { ...eventFilter, kind: kindFilter.value as Kind | "all" };
  renderSelectedSession();
});

cancelRunButton.addEventListener("click", async () => {
  if (!selectedSessionId) {
    return;
  }
  const sessionId = selectedSessionId;

  await runGuarded(async () => {
    await invoke("cancel_run", { request: { session_id: sessionId } });
    const current = sessions.get(sessionId);
    if (current) {
      sessions.set(sessionId, {
        ...current,
        lifecycle: "cancelling",
        status: titleFromLifecycle("cancelling"),
        running: true,
      });
      renderAll();
    }
  }, "Unable to cancel run.");
});

retryRunButton.addEventListener("click", async () => {
  if (!selectedSessionId) {
    return;
  }
  const sessionId = selectedSessionId;

  await runGuarded(async () => {
    const response = await invoke<StartRunResponse>("retry_run", {
      request: { session_id: sessionId },
    });
    setComposerMessage(`Retried ${sessionId} as ${response.session_id}.`);
  }, "Unable to retry run.");
});

previousSessionButton.addEventListener("click", async () => {
  await stepSelectedSession("previous");
});

nextSessionButton.addEventListener("click", async () => {
  await stepSelectedSession("next");
});

window.addEventListener("keydown", async (event) => {
  const target = event.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) {
    return;
  }

  if (event.key === "\\" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    sidebarCollapsed = !sidebarCollapsed;
    renderAll();
  }

  if (event.key === "[" || event.key === "ArrowUp") {
    event.preventDefault();
    await stepSelectedSession("previous");
  }

  if (event.key === "]" || event.key === "ArrowDown") {
    event.preventDefault();
    await stepSelectedSession("next");
  }
});

listen<AgentEvent>("agent-event", async (event) => {
  const existing = sessions.get(event.payload.session_id);
  if (existing) {
    sessions.set(existing.id, applyAgentEvent(existing, event.payload));
  } else {
    const summary = await invoke<SessionListItem | null>("get_session", {
      request: { session_id: event.payload.session_id },
    });
    if (summary) {
      upsertSession(summary);
    }
  }

  renderAll();
});

renderSidebarState();

bootstrap().catch((error) => {
  setError(error instanceof Error ? error.message : String(error));
  setLoadingState(false);
});
