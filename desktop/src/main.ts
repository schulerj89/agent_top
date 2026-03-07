import "./style.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Kind = "status" | "command" | "file" | "warning" | "error" | "note";

type Settings = {
  model: string;
  sandbox: string;
  approval: string;
};

type Bootstrap = {
  workspace: string;
  settings: Settings;
};

type AgentEvent = {
  session_id: string;
  timestamp: string;
  kind: Kind;
  message: string;
  finished: boolean;
};

type StartRunResponse = {
  session_id: string;
};

type SessionState = {
  id: string;
  prompt: string;
  workspace: string;
  status: string;
  running: boolean;
  events: number;
  commands: number;
  warnings: number;
  eventsElement: HTMLUListElement;
  statusElement: HTMLElement;
  metricsElement: HTMLElement;
  cardElement: HTMLElement;
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
        <p class="summary">Choose a workspace, launch multiple Codex sessions in parallel, and keep each run in its own live card.</p>
      </div>
      <div class="hero-meta">
        <div class="meta-card">
          <span>Active Runs</span>
          <strong id="activeRuns">0</strong>
        </div>
        <div class="meta-card">
          <span>Total Runs</span>
          <strong id="totalRuns">0</strong>
        </div>
        <div class="meta-card">
          <span>Total Events</span>
          <strong id="totalEvents">0</strong>
        </div>
        <div class="meta-card">
          <span>Total Warnings</span>
          <strong id="totalWarnings">0</strong>
        </div>
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

    <section class="grid">
      <section class="panel composer-panel">
        <header class="panel-header">
          <h2>Composer</h2>
          <p>Each launch creates a separate session card below.</p>
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
          <p>Add more sessions without replacing the current ones.</p>
        </header>
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
const sessionList = document.querySelector<HTMLDivElement>("#sessionList")!;

let currentWorkspace = "";
let sessionTotal = 0;
let globalEventTotal = 0;
let globalWarningTotal = 0;
let runningCount = 0;
const sessions = new Map<string, SessionState>();

function updateHeroStats() {
  activeRuns.textContent = String(runningCount);
  totalRuns.textContent = String(sessionTotal);
  totalEvents.textContent = String(globalEventTotal);
  totalWarnings.textContent = String(globalWarningTotal);
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

function createSessionCard(sessionId: string, prompt: string, workspace: string): SessionState {
  const card = document.createElement("article");
  card.className = "session-card";
  card.innerHTML = `
    <header class="session-header">
      <div>
        <span class="session-id">${sessionId}</span>
        <h3>${prompt === "/status" ? "/status" : "Prompt Run"}</h3>
      </div>
      <span class="session-status">Launching</span>
    </header>
    <p class="session-workspace"></p>
    <p class="session-prompt"></p>
    <div class="session-metrics">0 events · 0 commands · 0 warnings</div>
    <ul class="session-events"></ul>
  `;

  card.querySelector<HTMLElement>(".session-workspace")!.textContent = workspace;
  card.querySelector<HTMLElement>(".session-prompt")!.textContent = prompt;
  sessionList.prepend(card);

  return {
    id: sessionId,
    prompt,
    workspace,
    status: "Launching",
    running: true,
    events: 0,
    commands: 0,
    warnings: 0,
    eventsElement: card.querySelector<HTMLUListElement>(".session-events")!,
    statusElement: card.querySelector<HTMLElement>(".session-status")!,
    metricsElement: card.querySelector<HTMLElement>(".session-metrics")!,
    cardElement: card,
  };
}

function updateSessionSummary(session: SessionState) {
  session.statusElement.textContent = session.status;
  session.metricsElement.textContent = `${session.events} events · ${session.commands} commands · ${session.warnings} warnings`;
  session.cardElement.dataset.state = session.running ? "running" : "finished";
}

function appendSessionEvent(event: AgentEvent) {
  const session = sessions.get(event.session_id);
  if (!session) {
    return;
  }

  session.events += 1;
  if (event.kind === "command") {
    session.commands += 1;
  }
  if (event.kind === "warning" || event.kind === "error") {
    session.warnings += 1;
    globalWarningTotal += 1;
  }

  globalEventTotal += 1;
  session.status = event.finished ? "Completed" : event.kind === "status" ? event.message : "Running";

  const item = document.createElement("li");
  item.className = `event-item kind-${event.kind}`;
  item.innerHTML = `
    <span class="event-time">${event.timestamp}</span>
    <span class="event-kind">${event.kind}</span>
    <span class="event-message"></span>
  `;
  item.querySelector<HTMLElement>(".event-message")!.textContent = event.message;
  session.eventsElement.prepend(item);

  if (event.finished && session.running) {
    session.running = false;
    runningCount = Math.max(0, runningCount - 1);
  }

  updateSessionSummary(session);
  updateHeroStats();
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

  try {
    setComposerMessage("Starting Codex run...");
    const response = await invoke<StartRunResponse>("start_run", {
      request: {
        prompt: trimmedPrompt,
        workspace: currentWorkspace,
        settings: currentSettings(),
      },
    });

    sessionTotal += 1;
    runningCount += 1;
    const session = createSessionCard(response.session_id, trimmedPrompt, currentWorkspace);
    sessions.set(session.id, session);
    updateHeroStats();
    setComposerMessage(`Started ${response.session_id}.`);
  } catch (error) {
    setComposerMessage(error instanceof Error ? error.message : String(error));
  }
}

async function bootstrap() {
  const payload = await invoke<Bootstrap>("bootstrap");
  currentWorkspace = payload.workspace;
  workspaceLabel.textContent = payload.workspace;
  modelInput.value = payload.settings.model;
  sandboxInput.value = payload.settings.sandbox;
  approvalInput.value = payload.settings.approval;
  updateHeroStats();
}

chooseFolderButton.addEventListener("click", async () => {
  const selected = await invoke<string | null>("pick_workspace");

  if (typeof selected === "string") {
    currentWorkspace = selected;
    workspaceLabel.textContent = selected;
    setComposerMessage("Workspace updated.");
  }
});

addRunButton.addEventListener("click", async () => {
  await startRun(promptInput.value);
});

statusButton.addEventListener("click", async () => {
  await startRun("/status");
});

listen<AgentEvent>("agent-event", (event) => {
  appendSessionEvent(event.payload);
});

bootstrap().catch((error) => {
  setComposerMessage(error instanceof Error ? error.message : String(error));
});
