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
  timestamp: string;
  kind: Kind;
  message: string;
  finished: boolean;
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
        <p class="summary">Run Codex from a desktop shell, tune settings, and watch the event stream in real time.</p>
      </div>
      <div class="hero-meta">
        <div class="meta-card">
          <span>Status</span>
          <strong id="statusText">Idle</strong>
        </div>
        <div class="meta-card">
          <span>Events</span>
          <strong id="eventsCount">0</strong>
        </div>
        <div class="meta-card">
          <span>Commands</span>
          <strong id="commandsCount">0</strong>
        </div>
        <div class="meta-card">
          <span>Warnings</span>
          <strong id="warningsCount">0</strong>
        </div>
      </div>
    </section>

    <section class="grid">
      <section class="panel control-panel">
        <header class="panel-header">
          <h2>Run</h2>
          <p>Launch a Codex session from the desktop shell.</p>
        </header>
        <label class="field">
          <span>Prompt</span>
          <textarea id="promptInput" rows="6" placeholder="Ask Codex to inspect a repo, summarize a file, or execute a focused task."></textarea>
        </label>
        <label class="field">
          <span>Workspace</span>
          <input id="workspaceInput" type="text" />
        </label>
        <div class="actions">
          <button id="runButton" class="primary">Start Run</button>
          <span id="runMessage" class="run-message">Ready.</span>
        </div>
      </section>

      <section class="panel settings-panel">
        <header class="panel-header">
          <h2>Settings</h2>
          <p>These values are passed to the real Codex CLI launch.</p>
        </header>
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
      </section>
    </section>

    <section class="panel event-panel">
      <header class="panel-header">
        <h2>Live Feed</h2>
        <p>Incoming session events from the Rust backend.</p>
      </header>
      <ul id="eventList" class="event-list"></ul>
    </section>
  </main>
`;

const promptInput = document.querySelector<HTMLTextAreaElement>("#promptInput")!;
const workspaceInput = document.querySelector<HTMLInputElement>("#workspaceInput")!;
const modelInput = document.querySelector<HTMLInputElement>("#modelInput")!;
const sandboxInput = document.querySelector<HTMLSelectElement>("#sandboxInput")!;
const approvalInput = document.querySelector<HTMLSelectElement>("#approvalInput")!;
const runButton = document.querySelector<HTMLButtonElement>("#runButton")!;
const runMessage = document.querySelector<HTMLSpanElement>("#runMessage")!;
const statusText = document.querySelector<HTMLElement>("#statusText")!;
const eventsCount = document.querySelector<HTMLElement>("#eventsCount")!;
const commandsCount = document.querySelector<HTMLElement>("#commandsCount")!;
const warningsCount = document.querySelector<HTMLElement>("#warningsCount")!;
const eventList = document.querySelector<HTMLUListElement>("#eventList")!;

let running = false;
let totalEvents = 0;
let totalCommands = 0;
let totalWarnings = 0;

const appendEvent = (event: AgentEvent) => {
  totalEvents += 1;
  if (event.kind === "command") {
    totalCommands += 1;
  }
  if (event.kind === "warning" || event.kind === "error") {
    totalWarnings += 1;
  }

  eventsCount.textContent = String(totalEvents);
  commandsCount.textContent = String(totalCommands);
  warningsCount.textContent = String(totalWarnings);
  statusText.textContent = event.finished ? "Completed" : event.kind === "status" ? event.message : "Running";

  const item = document.createElement("li");
  item.className = `event-item kind-${event.kind}`;
  item.innerHTML = `
    <span class="event-time">${event.timestamp}</span>
    <span class="event-kind">${event.kind}</span>
    <span class="event-message"></span>
  `;
  item.querySelector(".event-message")!.textContent = event.message;
  eventList.prepend(item);

  if (event.finished) {
    running = false;
    runButton.disabled = false;
    runMessage.textContent = "Run completed.";
  }
};

const bootstrap = async () => {
  const payload = await invoke<Bootstrap>("bootstrap");
  workspaceInput.value = payload.workspace;
  modelInput.value = payload.settings.model;
  sandboxInput.value = payload.settings.sandbox;
  approvalInput.value = payload.settings.approval;
};

const clearMetrics = () => {
  totalEvents = 0;
  totalCommands = 0;
  totalWarnings = 0;
  eventsCount.textContent = "0";
  commandsCount.textContent = "0";
  warningsCount.textContent = "0";
  eventList.innerHTML = "";
};

runButton.addEventListener("click", async () => {
  if (running) {
    return;
  }

  const prompt = promptInput.value.trim();
  if (!prompt) {
    runMessage.textContent = "Prompt is required.";
    return;
  }

  clearMetrics();
  running = true;
  runButton.disabled = true;
  runMessage.textContent = "Starting Codex run...";
  statusText.textContent = "Launching";

  try {
    await invoke("start_run", {
      request: {
        prompt,
        workspace: workspaceInput.value.trim(),
        settings: {
          model: modelInput.value.trim(),
          sandbox: sandboxInput.value,
          approval: approvalInput.value,
        },
      },
    });
  } catch (error) {
    running = false;
    runButton.disabled = false;
    statusText.textContent = "Error";
    runMessage.textContent = error instanceof Error ? error.message : String(error);
  }
});

listen<AgentEvent>("agent-event", (event) => {
  appendEvent(event.payload);
});

bootstrap().catch((error) => {
  runMessage.textContent = error instanceof Error ? error.message : String(error);
  statusText.textContent = "Bootstrap error";
});
