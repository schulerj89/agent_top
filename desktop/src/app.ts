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
import type { AppDom } from "./dom";
import { loadTauriApi, type TauriApi } from "./tauri_api";

type Settings = {
  model: string;
  sandbox: string;
  approval: string;
  bypass_approvals_and_sandbox: boolean;
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

type CancelRunResponse = {
  session: SessionListItem | null;
};

export function startApp(dom: AppDom) {
  const sessions = new Map<string, SessionState>();
  let tauriApi: TauriApi | null = null;
  let currentWorkspace = "";
  let loading = true;
  let loadingDetail = false;
  let selectedSessionId: string | null = null;
  let draftingNewSession = false;
  let navSearch = "";
  let eventFilter: SessionFilter = { query: "", kind: "all" };
  let defaultSettings: Settings = {
    model: "",
    sandbox: "workspace-write",
    approval: "never",
    bypass_approvals_and_sandbox: false,
  };
  let defaultWorkspace = "";

  function updateHeroStats() {
    const values = [...sessions.values()];
    dom.activeRuns.textContent = String(values.filter((session) => session.running).length);
    dom.totalRuns.textContent = String(values.length);
    dom.totalEvents.textContent = String(values.reduce((sum, session) => sum + session.totalEvents, 0));
    dom.totalWarnings.textContent = String(values.reduce((sum, session) => sum + session.warnings, 0));
  }

  function currentSettings(): Settings {
    return {
      model: dom.modelInput.value.trim(),
      sandbox: dom.sandboxInput.value,
      approval: dom.approvalInput.value,
      bypass_approvals_and_sandbox: dom.bypassInput.checked,
    };
  }

  function setComposerMessage(message: string) {
    dom.composerMessage.textContent = message;
  }

  function setError(message: string | null) {
    if (!message) {
      dom.errorBanner.classList.add("hidden");
      dom.errorBanner.textContent = "";
      return;
    }

    dom.errorBanner.classList.remove("hidden");
    dom.errorBanner.textContent = message;
  }

  function setLoadingState(isLoading: boolean) {
    loading = isLoading;
    dom.chooseFolderButton.disabled = isLoading;
    dom.addRunButton.disabled = isLoading;
    dom.newSessionButton.disabled = isLoading;
    syncSettingsControls();
  }

  function syncSettingsControls() {
    const bypass = dom.bypassInput.checked;
    dom.sandboxInput.disabled = loading || bypass;
    dom.approvalInput.disabled = loading || bypass;
    dom.settingsNote.innerHTML = bypass
      ? "<code>Bypass approvals and sandbox</code> is enabled. Codex will receive <code>--dangerously-bypass-approvals-and-sandbox</code> and ignore the sandbox and approval selectors."
      : "<code>Danger full access</code> still uses Codex sandbox flags. Enable bypass to send <code>--dangerously-bypass-approvals-and-sandbox</code> and ignore sandbox and approval.";
  }

  function applyComposerState(workspace: string, prompt: string, settings: SessionSettings | Settings) {
    currentWorkspace = workspace;
    dom.workspaceLabel.textContent = workspace || "No workspace selected";
    dom.promptInput.value = prompt;
    dom.modelInput.value = settings.model;
    dom.sandboxInput.value = settings.sandbox;
    dom.approvalInput.value = settings.approval;
    dom.bypassInput.checked = settings.bypass_approvals_and_sandbox;
    syncSettingsControls();
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
    dom.sessionNav.replaceChildren(
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
      applyComposerState(defaultWorkspace, dom.promptInput.value, defaultSettings);
      dom.detailTitle.textContent = "No session selected";
      dom.detailSubtitle.textContent = draftingNewSession
        ? "Compose a prompt to start a new session."
        : "Choose a session from the left rail.";
      dom.detailStatus.textContent = "-";
      dom.detailEvents.textContent = "0";
      dom.detailCommands.textContent = "0";
      dom.detailWarnings.textContent = "0";
      dom.detailWorkspace.textContent = "";
      dom.detailCodexSession.textContent = "";
      dom.detailPrompt.textContent = "";
      dom.detailLatest.textContent = "";
      dom.detailMessage.textContent = "Session details load on demand.";
      dom.detailEventsList.replaceChildren();
      dom.cancelRunButton.disabled = true;
      dom.retryRunButton.disabled = true;
      dom.deleteSessionButton.disabled = true;
      return;
    }

    dom.detailTitle.textContent = session.title;
    dom.detailSubtitle.textContent = `${session.id} | ${session.updatedAt}`;
    dom.detailStatus.textContent = session.status;
    dom.detailEvents.textContent = String(session.totalEvents);
    dom.detailCommands.textContent = String(session.commands);
    dom.detailWarnings.textContent = String(session.warnings);
    dom.detailWorkspace.textContent = session.workspace;
    dom.detailCodexSession.textContent = session.codexSessionId
      ? `Codex session: ${session.codexSessionId}`
      : "Codex session: not captured yet";
    dom.detailPrompt.textContent = session.prompt;
    dom.detailLatest.textContent = `Latest: ${session.latestMessage}`;
    dom.detailMessage.textContent = loadingDetail
      ? "Loading session events..."
      : session.eventsLoaded
        ? "Session events are loaded from SQLite."
        : "Select a session to load its events.";
    dom.cancelRunButton.disabled = !session.running;
    dom.retryRunButton.disabled = session.running;
    dom.deleteSessionButton.disabled = session.running;

    const visibleEvents = session.eventsLoaded ? filterSessionEvents(session, eventFilter).slice(-100).reverse() : [];
    dom.detailEventsList.replaceChildren(
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
    if (!tauriApi) {
      return;
    }

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
      const api = tauriApi;
      if (!api) {
        throw new Error("Tauri runtime is not available. Launch this UI through the desktop app.");
      }
      loadingDetail = true;
      renderSelectedSession();
      const events = await api.invoke<SessionEvent[]>("get_session_events", {
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
    if (!selectedSessionId || !tauriApi) {
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
      const api = tauriApi;
      if (!api) {
        throw new Error("Tauri runtime is not available. Launch this UI through the desktop app.");
      }
      const response = await api.invoke<DeleteSessionResponse>("delete_session", {
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
      if (!tauriApi) {
        throw new Error("Tauri runtime is not available. Launch this UI through the desktop app.");
      }
      const api = tauriApi;
      setLoadingState(true);
      const settings = currentSettings();
      const existing = selectedSessionId && !draftingNewSession ? sessions.get(selectedSessionId) ?? null : null;

      if (existing) {
        if (existing.running) {
          throw new Error("session is already running");
        }
        const nextCodexSessionId =
          existing.workspace === currentWorkspace ? existing.codexSessionId : null;

        setComposerMessage(`Continuing ${existing.id}...`);
        const response = await api.invoke<StartRunResponse>("continue_session", {
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
          codex_session_id: nextCodexSessionId,
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
      const response = await api.invoke<StartRunResponse>("start_run", {
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
        codex_session_id: null,
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
      tauriApi = await loadTauriApi();
      if (!tauriApi) {
        defaultWorkspace = "Desktop runtime required";
        applyComposerState(defaultWorkspace, "", defaultSettings);
        setError("Tauri runtime is not available in the browser preview. Open Agent Top as the desktop app to run sessions.");
        setComposerMessage("Browser preview mode. Tauri commands are disabled.");
        renderAll();
        return;
      }

      const payload = await tauriApi.invoke<Bootstrap>("bootstrap");
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

  dom.chooseFolderButton.addEventListener("click", async () => {
    if (loading) {
      return;
    }

    await runGuarded(async () => {
      dom.chooseFolderButton.disabled = true;
      if (!tauriApi) {
        throw new Error("Tauri runtime is not available. Launch this UI through the desktop app.");
      }
      const selected = await tauriApi.invoke<string | null>("pick_workspace");
      if (typeof selected === "string" && selected.trim()) {
        defaultWorkspace = selected;
        currentWorkspace = selected;
        dom.workspaceLabel.textContent = selected;
        setComposerMessage("Workspace updated.");
      }
    }, "Unable to choose workspace.");
    dom.chooseFolderButton.disabled = false;
  });

  dom.addRunButton.addEventListener("click", async () => {
    await startRun(dom.promptInput.value);
  });

  dom.newSessionButton.addEventListener("click", () => {
    beginNewSessionDraft();
    renderAll();
    setComposerMessage("Ready to start a new session.");
  });

  dom.navSearchInput.addEventListener("input", () => {
    navSearch = dom.navSearchInput.value;
    renderSessionNav();
  });

  dom.eventSearchInput.addEventListener("input", () => {
    eventFilter = { ...eventFilter, query: dom.eventSearchInput.value };
    renderSelectedSession();
  });

  dom.kindFilter.addEventListener("change", () => {
    eventFilter = { ...eventFilter, kind: dom.kindFilter.value as Kind | "all" };
    renderSelectedSession();
  });

  dom.bypassInput.addEventListener("change", () => {
    syncSettingsControls();
  });

  dom.cancelRunButton.addEventListener("click", async () => {
    if (!selectedSessionId) {
      return;
    }
    const sessionId = selectedSessionId;

    await runGuarded(async () => {
      if (!tauriApi) {
        throw new Error("Tauri runtime is not available. Launch this UI through the desktop app.");
      }
      const response = await tauriApi.invoke<CancelRunResponse>("cancel_run", {
        request: { session_id: sessionId },
      });
      if (response.session) {
        upsertSession(response.session);
        renderAll();
        return;
      }

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

  dom.retryRunButton.addEventListener("click", async () => {
    if (!selectedSessionId) {
      return;
    }
    const sessionId = selectedSessionId;

    await runGuarded(async () => {
      if (!tauriApi) {
        throw new Error("Tauri runtime is not available. Launch this UI through the desktop app.");
      }
      const response = await tauriApi.invoke<StartRunResponse>("retry_run", {
        request: { session_id: sessionId },
      });
      setComposerMessage(`Retried ${sessionId} as ${response.session_id}.`);
    }, "Unable to retry run.");
  });

  dom.deleteSessionButton.addEventListener("click", async () => {
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

  bootstrap().catch((error) => {
    setError(error instanceof Error ? error.message : String(error));
    setLoadingState(false);
  });

  loadTauriApi()
    .then(async (api) => {
      tauriApi = api;
      if (!tauriApi) {
        return;
      }

      await tauriApi.listen<AgentEvent>("agent-event", async (event) => {
        const existing = sessions.get(event.payload.session_id);
        if (existing) {
          sessions.set(existing.id, applyAgentEvent(existing, event.payload));
        } else {
          const summary = await tauriApi!.invoke<SessionListItem | null>("get_session", {
            request: { session_id: event.payload.session_id },
          });
          if (summary) {
            upsertSession(summary);
          }
        }

        renderAll();
      });
    })
    .catch(() => {
      tauriApi = null;
    });
}
