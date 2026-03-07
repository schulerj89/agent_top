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
  type SessionSettings,
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

type DeleteSessionResponse = {
  deleted: boolean;
};

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("app root not found");
}

app.innerHTML = `
  <main class="app-shell">
    <aside id="sidebarRail" class="sidebar-rail">
      <div class="rail-header">
        <div class="rail-brand">
          <p class="eyebrow">Desktop Monitor</p>
          <h1 class="rail-title">Agent Top</h1>
        </div>
      </div>

      <div class="rail-search-wrap">
        <button id="newSessionButton" class="primary rail-new-session" type="button">New Session</button>
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
          <p class="summary">Use the left rail like a real session navigator, keep the selected session as the primary workspace, and prune completed runs when you no longer need them.</p>
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
              <button id="cancelRunButton" class="ghost">Cancel</button>
              <button id="retryRunButton" class="ghost">Retry</button>
              <button id="deleteSessionButton" class="ghost">Delete</button>
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
const newSessionButton = document.querySelector<HTMLButtonElement>("#newSessionButton")!;
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
const cancelRunButton = document.querySelector<HTMLButtonElement>("#cancelRunButton")!;
const retryRunButton = document.querySelector<HTMLButtonElement>("#retryRunButton")!;
const deleteSessionButton = document.querySelector<HTMLButtonElement>("#deleteSessionButton")!;
const eventSearchInput = document.querySelector<HTMLInputElement>("#eventSearchInput")!;
const kindFilter = document.querySelector<HTMLSelectElement>("#kindFilter")!;
const detailEventsList = document.querySelector<HTMLUListElement>("#detailEventsList")!;

let currentWorkspace = "";
let loading = true;
let loadingDetail = false;
let selectedSessionId: string | null = null;
let draftingNewSession = false;
const sessions = new Map<string, SessionState>();
let navSearch = "";
let eventFilter: SessionFilter = { query: "", kind: "all" };
let defaultSettings: Settings = { model: "", sandbox: "workspace-write", approval: "never" };
let defaultWorkspace = "";

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
  newSessionButton.disabled = isLoading;
}

function applyComposerState(workspace: string, prompt: string, settings: SessionSettings | Settings) {
  currentWorkspace = workspace;
  workspaceLabel.textContent = workspace || "No workspace selected";
  promptInput.value = prompt;
  modelInput.value = settings.model;
  sandboxInput.value = settings.sandbox;
  approvalInput.value = settings.approval;
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

  if (draftingNewSession) {
    return;
  }

  selectedSessionId = pickInitialSessionId([...sessions.values()]);
}

function beginNewSessionDraft() {
  draftingNewSession = true;
  selectedSessionId = null;
  applyComposerState(defaultWorkspace, "", defaultSettings);
}

function promptTitle(prompt: string): string {
  const words = prompt.trim().split(/\s+/).filter(Boolean);
  const short = words.slice(0, 5).join(" ");
  return short || "Untitled session";
}

function renderSessionNav() {
  const visible = sortedVisibleSessions();
  sessionNav.replaceChildren(
    ...(visible.length > 0
      ? visible.map((session) => {
          const navTitle = promptTitle(session.prompt);
          const button = document.createElement("button");
          button.type = "button";
          button.className = "session-nav-item";
          button.dataset.running = String(session.running);
          if (session.id === selectedSessionId) {
            button.dataset.active = "true";
          }
          button.title = navTitle;
          button.innerHTML = `
            <span class="session-nav-title"></span>
          `;
          button.querySelector<HTMLElement>(".session-nav-title")!.textContent = navTitle;

          button.addEventListener("click", async () => {
            await selectSession(session.id);
          });
          return button;
        })
      : [
          Object.assign(document.createElement("p"), {
            className: "empty-sessions",
            textContent: "No sessions match the current search.",
          }),
        ]),
  );
}

function renderSelectedSession() {
  const session = selectedSessionId ? sessions.get(selectedSessionId) ?? null : null;

  if (!session) {
    applyComposerState(defaultWorkspace, promptInput.value, defaultSettings);
    detailTitle.textContent = "No session selected";
    detailSubtitle.textContent = draftingNewSession
      ? "Compose a prompt to start a new session."
      : "Choose a session from the left rail.";
    detailStatus.textContent = "-";
    detailEvents.textContent = "0";
    detailCommands.textContent = "0";
    detailWarnings.textContent = "0";
    detailWorkspace.textContent = "";
    detailPrompt.textContent = "";
    detailLatest.textContent = "";
    detailMessage.textContent = "Session details load on demand.";
    detailEventsList.replaceChildren();
    cancelRunButton.disabled = true;
    retryRunButton.disabled = true;
    deleteSessionButton.disabled = true;
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
  cancelRunButton.disabled = !session.running;
  retryRunButton.disabled = session.running;
  deleteSessionButton.disabled = session.running;

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
  draftingNewSession = false;
  selectedSessionId = sessionId;
  const selected = sessions.get(sessionId);
  if (selected) {
    applyComposerState(selected.workspace, selected.prompt, selected.settings);
  }
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

async function deleteSelectedSession() {
  if (!selectedSessionId) {
    return;
  }

  const sessionId = selectedSessionId;
  const session = sessions.get(sessionId);
  if (!session || session.running) {
    setComposerMessage("Stop the run before deleting the session.");
    return;
  }

  const confirmed = window.confirm(`Delete session "${promptTitle(session.prompt)}"?`);
  if (!confirmed) {
    return;
  }

  await runGuarded(async () => {
    const response = await invoke<DeleteSessionResponse>("delete_session", {
      request: { session_id: sessionId },
    });
    if (!response.deleted) {
      throw new Error("session history entry was not found");
    }

    sessions.delete(sessionId);
    selectedSessionId = pickInitialSessionId([...sessions.values()]);
    renderAll();
    if (selectedSessionId) {
      draftingNewSession = false;
      await selectSession(selectedSessionId);
    } else {
      beginNewSessionDraft();
      renderAll();
    }
    setComposerMessage(`Deleted ${sessionId}.`);
  }, "Unable to delete session.");
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
    const settings = currentSettings();
    const existing = selectedSessionId && !draftingNewSession ? sessions.get(selectedSessionId) ?? null : null;

    if (existing) {
      if (existing.running) {
        throw new Error("session is already running");
      }

      setComposerMessage(`Continuing ${existing.id}...`);
      const response = await invoke<StartRunResponse>("continue_session", {
        request: { session_id: existing.id, limit: null },
        run: {
          prompt: trimmedPrompt,
          workspace: currentWorkspace,
          settings,
        },
      });

      upsertSession({
        session_id: response.session_id,
        title: existing.title,
        prompt: trimmedPrompt,
        workspace: currentWorkspace,
        lifecycle: "launching",
        status: "Launching",
        updated_at: String(Date.now()),
        last_event_at: existing.updatedAt,
        last_message: "waiting for first event",
        total_events: existing.totalEvents,
        command_count: existing.commands,
        warning_count: existing.warnings,
        error_count: 0,
        settings,
      });

      selectedSessionId = response.session_id;
      draftingNewSession = false;
      renderAll();
      setComposerMessage(`Continued ${response.session_id}.`);
      return;
    }

    setComposerMessage("Starting new session...");
    const response = await invoke<StartRunResponse>("start_run", {
      request: {
        prompt: trimmedPrompt,
        workspace: currentWorkspace,
        settings,
      },
    });

    upsertSession({
      session_id: response.session_id,
      title: promptTitle(trimmedPrompt),
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
      settings,
    });

    selectedSessionId = response.session_id;
    draftingNewSession = false;
    renderAll();
    setComposerMessage(`Started ${response.session_id}.`);
  }, "Unable to start run.");

  setLoadingState(false);
}

async function bootstrap() {
  await runGuarded(async () => {
    setLoadingState(true);
    const payload = await invoke<Bootstrap>("bootstrap");
    defaultWorkspace = payload.workspace;
    defaultSettings = payload.settings;
    applyComposerState(payload.workspace, "", payload.settings);

    sessions.clear();
    for (const summary of payload.sessions) {
      upsertSession(summary);
    }

    ensureSelection();
    renderAll();

    if (selectedSessionId) {
      draftingNewSession = false;
      await selectSession(selectedSessionId);
    }

    setComposerMessage(`Loaded ${payload.sessions.length} persisted sessions.`);
  }, "Unable to bootstrap desktop state.");

  setLoadingState(false);
}

chooseFolderButton.addEventListener("click", async () => {
  if (loading) {
    return;
  }

  await runGuarded(async () => {
    chooseFolderButton.disabled = true;
    const selected = await invoke<string | null>("pick_workspace");
    if (typeof selected === "string" && selected.trim()) {
      defaultWorkspace = selected;
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

newSessionButton.addEventListener("click", () => {
  beginNewSessionDraft();
  renderAll();
  setComposerMessage("Ready to start a new session.");
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

deleteSessionButton.addEventListener("click", async () => {
  await deleteSelectedSession();
});

window.addEventListener("keydown", async (event) => {
  const target = event.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) {
    return;
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

bootstrap().catch((error) => {
  setError(error instanceof Error ? error.message : String(error));
  setLoadingState(false);
});
