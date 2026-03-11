const MODEL_OPTIONS = [
  { value: "", label: "CLI default" },
  { value: "gpt-5.2-codex", label: "GPT-5.2 Codex" },
  { value: "gpt-5.1-codex", label: "GPT-5.1 Codex" },
  { value: "gpt-5.1-codex-mini", label: "GPT-5.1 Codex Mini" },
  { value: "gpt-5.1-codex-max", label: "GPT-5.1 Codex Max" },
  { value: "gpt-5-codex", label: "GPT-5 Codex" },
];

export const APP_SHELL = `
  <main class="app-shell">
    <aside id="sidebarRail" class="sidebar-rail">
      <div class="rail-header">
        <div class="rail-brand">
          <p class="eyebrow">Codex Desktop</p>
          <h1 class="rail-title">Threads</h1>
          <p class="rail-summary">Keep Codex runs organized as persistent threads with resumable history.</p>
        </div>
      </div>

      <nav class="rail-actions" aria-label="Primary">
        <button id="newSessionButton" class="rail-action rail-action-primary" type="button">New Thread</button>
        <button class="rail-action" type="button" disabled>Automations</button>
        <button class="rail-action" type="button" disabled>Skills</button>
      </nav>

      <div class="rail-search-wrap">
        <div class="rail-section-label">Recent Threads</div>
        <label class="field rail-search-field">
          <span>Search</span>
          <input id="navSearchInput" type="text" placeholder="Search threads, prompts, workspace" />
        </label>
      </div>

      <div id="sessionNav" class="session-nav"></div>

      <div class="rail-footer">
        <p>Shortcuts: <span>[</span> previous, <span>]</span> next</p>
        <p>Thread history is stored locally in SQLite.</p>
      </div>
    </aside>

    <div class="shell-separator"></div>

    <section class="content-shell">
      <section class="overview-strip panel">
        <div class="overview-copy">
          <p class="panel-kicker">Workspace</p>
          <strong id="workspaceLabel">Loading...</strong>
        </div>
        <div class="overview-metrics">
          <div class="meta-card compact"><span>Active</span><strong id="activeRuns">0</strong></div>
          <div class="meta-card compact"><span>Threads</span><strong id="totalRuns">0</strong></div>
          <div class="meta-card compact"><span>Events</span><strong id="totalEvents">0</strong></div>
          <div class="meta-card compact"><span>Warnings</span><strong id="totalWarnings">0</strong></div>
        </div>
        <div class="workspace-actions">
          <button id="chooseFolderButton" class="ghost">Choose Folder</button>
        </div>
      </section>

      <section id="errorBanner" class="error-banner hidden" role="alert"></section>

      <section class="workspace-stack">
        <section class="panel detail-panel">
          <header class="detail-header">
            <div>
              <p class="panel-kicker">Selected Thread</p>
              <h2 id="detailTitle">No thread selected</h2>
              <p id="detailSubtitle" class="detail-subtitle">Choose a thread from the left rail.</p>
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
            <p id="detailCodexSession" class="detail-codex-session"></p>
            <p id="detailPrompt" class="detail-prompt"></p>
            <p id="detailLatest" class="detail-latest"></p>
          </div>

          <div class="detail-scroll-shell">
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

            <p id="detailMessage" class="run-message">Thread details load on demand.</p>
            <ul id="detailEventsList" class="detail-events"></ul>
          </div>
        </section>

        <section class="panel composer-panel">
          <header class="panel-header composer-header">
            <div>
              <p class="panel-kicker">Composer</p>
              <h2>Continue this thread or start a new one</h2>
            </div>
            <button id="addRunButton" class="primary">Run With Codex</button>
          </header>
          <label class="field">
            <span>Prompt</span>
            <textarea id="promptInput" rows="5" placeholder="Ask Codex to inspect, build, or continue the selected thread."></textarea>
          </label>
          <div class="settings-grid">
            <label class="field">
              <span>Model</span>
              <select id="modelInput">
                ${MODEL_OPTIONS.map((model) => `<option value="${model.value}">${model.label}</option>`).join("")}
              </select>
            </label>
            <label class="field">
              <span>Sandbox</span>
              <select id="sandboxInput">
                <option value="read-only">Read-only</option>
                <option value="workspace-write">Workspace write</option>
                <option value="danger-full-access">Danger full access</option>
              </select>
            </label>
            <label class="field">
              <span>Approval</span>
              <select id="approvalInput">
                <option value="untrusted">Untrusted only</option>
                <option value="on-request">On request</option>
                <option value="never">Never ask</option>
              </select>
            </label>
            <label class="field checkbox-field">
              <span>Bypass</span>
              <span class="checkbox-control">
                <input id="bypassInput" type="checkbox" />
                <span>Bypass approvals and sandbox</span>
              </span>
            </label>
          </div>
          <p id="settingsNote" class="settings-note"><code>Danger full access</code> still uses Codex sandbox flags. Enable bypass to send <code>--dangerously-bypass-approvals-and-sandbox</code> and ignore sandbox and approval.</p>
          <p id="composerMessage" class="run-message">Ready.</p>
        </section>
      </section>
    </section>
  </main>
`;
