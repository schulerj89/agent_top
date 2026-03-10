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
            <p>Launch new runs with explicit Codex execution settings. New sessions appear in the left rail immediately.</p>
          </header>
          <label class="field">
            <span>Prompt</span>
            <textarea id="promptInput" rows="6" placeholder="Describe the task for Codex."></textarea>
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
            <p id="detailCodexSession" class="detail-codex-session"></p>
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
