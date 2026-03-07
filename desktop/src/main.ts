import "./style.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import {
  applyAgentEvent,
  createSessionState,
  filterSessionEvents,
  titleFromLifecycle,
  type AgentEvent,
  type Kind,
  type SessionFilter,
  type SessionRecord,
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
  sessions: SessionRecord[];
};

type StartRunResponse = {
  session_id: string;
};

type SessionDom = {
  cardElement: HTMLElement;
  statusElement: HTMLElement;
  metricsElement: HTMLElement;
  latestElement: HTMLElement;
  eventsElement: HTMLUListElement;
  toggleButton: HTMLButtonElement;
  detailsElement: HTMLElement;
  cancelButton: HTMLButtonElement;
  retryButton: HTMLButtonElement;
  analyticsElement: HTMLElement;
};

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("app root not found");
}

app.innerHTML = `
  <main class="shell">
    <section class="hero">
      <div>
        <p class="eyebrow">Desktop Monitor</p>
        <h1>agent_top</h1>
        <p class="summary">Persistent session history, cancellable runs, event analytics, and searchable session cards for active and restored Codex runs.</p>
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

    <section class="grid">
      <section class="panel composer-panel">
        <header class="panel-header">
          <h2>Composer</h2>
          <p>Validated launches only. Failed Tauri commands surface here instead of disappearing.</p>
        </header>
        <label class="field">
          <span>Prompt</span>
          <textarea id="promptInput" rows="6" placeholder="Describe the task for Codex."></textarea>
        </label>
        <div class="quick-actions">
          <button id="statusButton" class="ghost">Run /status</button>
        </div>
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

      <section class="panel sessions-panel">
        <header class="panel-header">
          <h2>Runs</h2>
          <p>History is restored on startup. Use search and kind filters to cut down noisy sessions.</p>
        </header>
        <div class="session-filters">
          <input id="searchInput" type="text" placeholder="Search timestamps, kinds, and messages" />
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
        <div id="sessionList" class="session-list"></div>
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
const statusButton = document.querySelector<HTMLButtonElement>("#statusButton")!;
const composerMessage = document.querySelector<HTMLElement>("#composerMessage")!;
const errorBanner = document.querySelector<HTMLElement>("#errorBanner")!;
const sessionList = document.querySelector<HTMLDivElement>("#sessionList")!;
const searchInput = document.querySelector<HTMLInputElement>("#searchInput")!;
const kindFilter = document.querySelector<HTMLSelectElement>("#kindFilter")!;

let currentWorkspace = "";
let loading = true;
const sessionState = new Map<string, SessionState>();
const sessionDom = new Map<string, SessionDom>();
const expandedSessions = new Set<string>();
let filter: SessionFilter = { query: "", kind: "all" };

function updateHeroStats() {
  const sessions = [...sessionState.values()];
  activeRuns.textContent = String(sessions.filter((session) => session.running).length);
  totalRuns.textContent = String(sessions.length);
  totalEvents.textContent = String(sessions.reduce((sum, session) => sum + session.events.length, 0));
  totalWarnings.textContent = String(sessions.reduce((sum, session) => sum + session.warnings, 0));
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
  statusButton.disabled = isLoading;
}

function ensureSessionDom(session: SessionState): SessionDom {
  const existing = sessionDom.get(session.id);
  if (existing) {
    return existing;
  }

  const card = document.createElement("article");
  card.className = "session-card";
  card.innerHTML = `
    <header class="session-header">
      <div>
        <span class="session-id">${session.id}</span>
        <h3>${session.prompt === "/status" ? "/status" : "Prompt Run"}</h3>
      </div>
      <div class="session-header-actions">
        <span class="session-status">${session.status}</span>
        <button class="toggle-button" type="button">Expand</button>
      </div>
    </header>
    <p class="session-workspace"></p>
    <p class="session-prompt"></p>
    <p class="session-latest"></p>
    <div class="session-metrics"></div>
    <div class="session-actions">
      <button class="ghost cancel-button" type="button">Cancel</button>
      <button class="ghost retry-button" type="button">Retry</button>
    </div>
    <div class="session-analytics"></div>
    <div class="session-details is-collapsed">
      <ul class="session-events"></ul>
    </div>
  `;

  card.querySelector<HTMLElement>(".session-workspace")!.textContent = session.workspace;
  card.querySelector<HTMLElement>(".session-prompt")!.textContent = session.prompt;
  sessionList.prepend(card);

  const dom: SessionDom = {
    cardElement: card,
    statusElement: card.querySelector<HTMLElement>(".session-status")!,
    metricsElement: card.querySelector<HTMLElement>(".session-metrics")!,
    latestElement: card.querySelector<HTMLElement>(".session-latest")!,
    eventsElement: card.querySelector<HTMLUListElement>(".session-events")!,
    toggleButton: card.querySelector<HTMLButtonElement>(".toggle-button")!,
    detailsElement: card.querySelector<HTMLElement>(".session-details")!,
    cancelButton: card.querySelector<HTMLButtonElement>(".cancel-button")!,
    retryButton: card.querySelector<HTMLButtonElement>(".retry-button")!,
    analyticsElement: card.querySelector<HTMLElement>(".session-analytics")!,
  };

  dom.toggleButton.addEventListener("click", () => {
    if (expandedSessions.has(session.id)) {
      expandedSessions.delete(session.id);
    } else {
      expandedSessions.add(session.id);
    }
    renderSession(session.id);
  });

  dom.cancelButton.addEventListener("click", async () => {
    await runGuarded(async () => {
      await invoke("cancel_run", { request: { session_id: session.id } });
      const current = sessionState.get(session.id);
      if (current) {
        sessionState.set(session.id, {
          ...current,
          lifecycle: "cancelling",
          status: titleFromLifecycle("cancelling"),
          running: true,
        });
        renderSession(session.id);
        updateHeroStats();
      }
    }, "Unable to cancel run.");
  });

  dom.retryButton.addEventListener("click", async () => {
    await runGuarded(async () => {
      const response = await invoke<StartRunResponse>("retry_run", { request: { session_id: session.id } });
      setComposerMessage(`Retried ${session.id} as ${response.session_id}.`);
    }, "Unable to retry run.");
  });

  sessionDom.set(session.id, dom);
  return dom;
}

function renderSession(sessionId: string) {
  const state = sessionState.get(sessionId);
  if (!state) {
    return;
  }

  const dom = ensureSessionDom(state);
  dom.statusElement.textContent = state.status;
  dom.metricsElement.textContent = `${state.events.length} events - ${state.commands} commands - ${state.warnings} warnings`;
  dom.latestElement.textContent = `Latest: ${state.latestMessage}`;
  dom.cardElement.dataset.state = state.lifecycle;
  dom.cancelButton.disabled = !state.running;
  dom.retryButton.disabled = state.running;

  const expanded = expandedSessions.has(sessionId);
  dom.detailsElement.classList.toggle("is-collapsed", !expanded);
  dom.toggleButton.textContent = expanded ? "Collapse" : "Expand";

  const visibleEvents = filterSessionEvents(state, filter).slice(-50).reverse();
  dom.eventsElement.replaceChildren(
    ...(
      visibleEvents.length > 0
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
        : [Object.assign(document.createElement("li"), { className: "empty-events", textContent: "No matching events." })]
    ),
  );

  const lastCommand = [...state.events].reverse().find((event) => event.kind === "command");
  const fileEvents = state.events.filter((event) => event.kind === "file").length;
  dom.analyticsElement.textContent = `Lifecycle: ${titleFromLifecycle(state.lifecycle)} | Files: ${fileEvents} | Last command: ${lastCommand?.message ?? "none"}`;
}

function renderAllSessions() {
  for (const sessionId of sessionState.keys()) {
    renderSession(sessionId);
  }
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

    const nextSession: SessionState = {
      id: response.session_id,
      prompt: trimmedPrompt,
      workspace: currentWorkspace,
      status: "Launching",
      lifecycle: "launching",
      running: true,
      events: [],
      commands: 0,
      warnings: 0,
      latestMessage: "waiting for first event",
    };
    sessionState.set(nextSession.id, nextSession);
    expandedSessions.add(nextSession.id);
    renderSession(nextSession.id);
    updateHeroStats();
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

    sessionState.clear();
    for (const record of payload.sessions) {
      const session = createSessionState(record);
      sessionState.set(session.id, session);
    }

    renderAllSessions();
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

statusButton.addEventListener("click", async () => {
  await startRun("/status");
});

searchInput.addEventListener("input", () => {
  filter = { ...filter, query: searchInput.value };
  renderAllSessions();
});

kindFilter.addEventListener("change", () => {
  filter = { ...filter, kind: kindFilter.value as Kind | "all" };
  renderAllSessions();
});

listen<AgentEvent>("agent-event", (event) => {
  const current = sessionState.get(event.payload.session_id);
  if (!current) {
    return;
  }

  sessionState.set(current.id, applyAgentEvent(current, event.payload));
  renderSession(current.id);
  updateHeroStats();
});

bootstrap().catch((error) => {
  setError(error instanceof Error ? error.message : String(error));
  setLoadingState(false);
});
