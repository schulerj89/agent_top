use std::io;
use std::path::PathBuf;

use agent_top_core::{
    start_codex_run, Event, EventKind, ManagedRun, RunController, RunRequest, RunSettings,
    SessionLifecycle,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_state::{normalize_workspace_display, AppState, RunRequestPayload};
use crate::commands::EventPayload;
use crate::storage::{SessionStore, SessionUpdate, StoredSession, StoredThread};

pub fn validate_request(request: &RunRequestPayload) -> Result<(), String> {
    if request.prompt.trim().is_empty() {
        return Err("prompt cannot be empty".to_string());
    }

    let workspace = PathBuf::from(request.workspace.trim());
    if request.workspace.trim().is_empty() {
        return Err("workspace cannot be empty".to_string());
    }
    if !workspace.exists() {
        return Err("workspace does not exist".to_string());
    }
    if !workspace.is_dir() {
        return Err("workspace must be a directory".to_string());
    }

    Ok(())
}

pub fn register_run(
    state: &AppState,
    session_id: &str,
    controller: RunController,
) -> Result<(), String> {
    let mut guard = state
        .active_runs
        .lock()
        .map_err(|_| "active run state is unavailable".to_string())?;
    guard.insert(session_id.to_string(), controller);
    Ok(())
}

pub fn launch_run(
    app: AppHandle,
    state: &AppState,
    thread_id: String,
    run_id: String,
    prompt: String,
    workspace: String,
    settings: RunSettings,
    codex_session_id: Option<String>,
) -> Result<(), String> {
    let managed = start_codex_run(RunRequest {
        session_id: run_id.clone(),
        prompt,
        workspace,
        settings,
        codex_session_id,
    });

    register_run(state, &run_id, managed.controller.clone())?;
    forward_events(app, state.store.clone(), thread_id, run_id, managed);
    Ok(())
}

pub fn has_active_run(state: &AppState, thread_id: &str) -> Result<bool, String> {
    let Some(active_run_id) = state.store.selected_run_id_for_thread(thread_id)? else {
        return Ok(false);
    };
    let Some(thread) = state.store.get_thread(thread_id)? else {
        return Ok(false);
    };
    if thread.active_run_id.as_deref() != Some(active_run_id.as_str()) {
        return Ok(false);
    }
    let guard = state
        .active_runs
        .lock()
        .map_err(|_| "active run state is unavailable".to_string())?;
    Ok(guard.contains_key(&active_run_id) || thread.active_run_id.is_some())
}

pub fn next_session_seed(store: &SessionStore) -> Result<u64, String> {
    let highest = store
        .list_sessions(None)?
        .into_iter()
        .filter_map(|session| {
            session
                .id
                .strip_prefix("run-")
                .and_then(|value| value.parse::<u64>().ok())
        })
        .max()
        .unwrap_or(0);

    Ok(highest + 1)
}

pub fn next_thread_seed(store: &SessionStore) -> Result<u64, String> {
    let highest = store
        .list_threads(None)?
        .into_iter()
        .filter_map(|thread| {
            thread
                .id
                .strip_prefix("thread-")
                .and_then(|value| value.parse::<u64>().ok())
        })
        .max()
        .unwrap_or(0);

    Ok(highest + 1)
}

pub fn reconcile_orphaned_sessions(store: &SessionStore) -> Result<(), String> {
    for session in store.list_sessions(None)? {
        match session.lifecycle {
            SessionLifecycle::Launching | SessionLifecycle::Running => {
                store.update_session(
                    &session.id,
                    &SessionUpdate {
                        lifecycle: SessionLifecycle::Failed,
                        status: "Stopped before completion".to_string(),
                        last_message: None,
                    },
                )?;
            }
            SessionLifecycle::Cancelling => {
                store.update_session(
                    &session.id,
                    &SessionUpdate {
                        lifecycle: SessionLifecycle::Cancelled,
                        status: "Cancelled".to_string(),
                        last_message: None,
                    },
                )?;
            }
            SessionLifecycle::Cancelled | SessionLifecycle::Completed | SessionLifecycle::Failed => {}
        }
    }

    Ok(())
}

pub fn reconcile_cancelled_orphaned_session(
    store: &SessionStore,
    thread_id: &str,
) -> Result<Option<StoredThread>, String> {
    let Some(thread) = store.get_thread(thread_id)? else {
        return Err("session history entry not found".to_string());
    };

    match thread.lifecycle {
        SessionLifecycle::Launching | SessionLifecycle::Running | SessionLifecycle::Cancelling => {
            let target_run_id = thread
                .active_run_id
                .clone()
                .unwrap_or_else(|| thread.latest_run_id.clone());
            store.update_session(
                &target_run_id,
                &SessionUpdate {
                    lifecycle: SessionLifecycle::Cancelled,
                    status: "Cancelled".to_string(),
                    last_message: Some("session cancelled after losing the active process".to_string()),
                },
            )?;
            store.get_thread(thread_id)
        }
        SessionLifecycle::Cancelled | SessionLifecycle::Completed | SessionLifecycle::Failed => {
            Err("session is not running".to_string())
        }
    }
}

pub fn forward_events(
    app: AppHandle,
    store: SessionStore,
    thread_id: String,
    run_id: String,
    managed: ManagedRun,
) {
    std::thread::spawn(move || {
        while let Ok(update) = managed.receiver.recv() {
            let event = update.event.clone();
            let _ = persist_runner_update(&store, &run_id, &event, update.finished);
            let _ = app.emit(
                "agent-event",
                EventPayload::from_event(thread_id.clone(), run_id.clone(), event, update.finished),
            );

            if update.finished {
                let state: State<'_, AppState> = app.state();
                if let Ok(mut guard) = state.active_runs.lock() {
                    guard.remove(&run_id);
                }
                break;
            }
        }
    });
}

pub fn resume_codex_session_id(
    session: &StoredSession,
    requested_workspace: &str,
    requested_settings: &RunSettings,
) -> Option<String> {
    if session.total_events == 0 || !session.resume_ready {
        return None;
    }

    if session.settings != *requested_settings {
        return None;
    }

    if normalize_workspace_display(&session.workspace)
        == normalize_workspace_display(requested_workspace)
    {
        session.codex_session_id.clone()
    } else {
        None
    }
}

pub fn persist_runner_update(
    store: &SessionStore,
    session_id: &str,
    event: &Event,
    finished: bool,
) -> Result<(), String> {
    let payload_json = serde_json::to_string(event).ok();
    store.append_event(session_id, event, payload_json.as_deref())?;
    if let Some(codex_session_id) = extract_codex_session_id(event) {
        store.set_codex_session_id(session_id, &codex_session_id)?;
    }

    let lifecycle = lifecycle_for_event(event, finished);
    let status = if finished {
        status_for_finished_lifecycle(lifecycle).to_string()
    } else if event.kind == EventKind::Status {
        event.message.clone()
    } else {
        "Running".to_string()
    };

    store.update_session(
        session_id,
        &SessionUpdate {
            lifecycle,
            status,
            last_message: Some(event.message.clone()),
        },
    )?;
    if finished {
        store.set_resume_ready(session_id, lifecycle == SessionLifecycle::Completed)?;
    }

    Ok(())
}

pub fn lifecycle_for_event(event: &Event, finished: bool) -> SessionLifecycle {
    if finished {
        match event.kind {
            EventKind::Status => SessionLifecycle::Completed,
            EventKind::Warning if event.message.contains("cancelled") => {
                SessionLifecycle::Cancelled
            }
            _ => SessionLifecycle::Failed,
        }
    } else {
        SessionLifecycle::Running
    }
}

pub fn status_for_finished_lifecycle(lifecycle: SessionLifecycle) -> &'static str {
    match lifecycle {
        SessionLifecycle::Launching => "Launching",
        SessionLifecycle::Running => "Running",
        SessionLifecycle::Cancelling => "Cancelling",
        SessionLifecycle::Cancelled => "Cancelled",
        SessionLifecycle::Completed => "Completed",
        SessionLifecycle::Failed => "Failed",
    }
}

pub fn extract_codex_session_id(event: &Event) -> Option<String> {
    event.message
        .strip_prefix("thread started: ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn startup_state() -> Result<AppState, io::Error> {
    let store = SessionStore::new(crate::storage::default_db_path());
    store.init().map_err(io::Error::other)?;
    reconcile_orphaned_sessions(&store).map_err(io::Error::other)?;
    let next_session_id = next_session_seed(&store).map_err(io::Error::other)?;
    let next_thread_id = next_thread_seed(&store).map_err(io::Error::other)?;

    Ok(AppState {
        default_workspace: crate::app_state::detect_workspace(),
        store,
        next_session_id: std::sync::atomic::AtomicU64::new(next_session_id),
        next_thread_id: std::sync::atomic::AtomicU64::new(next_thread_id),
        active_runs: std::sync::Mutex::new(std::collections::HashMap::new()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::SettingsPayload;
    use crate::storage::CreateSessionInput;
    use tempfile::tempdir;

    fn make_store() -> (tempfile::TempDir, SessionStore) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sessions.sqlite3");
        let store = SessionStore::new(path);
        store.init().expect("init");
        (dir, store)
    }

    fn valid_request(workspace: String) -> RunRequestPayload {
        RunRequestPayload {
            prompt: "fix the tests".to_string(),
            workspace,
            settings: SettingsPayload::default(),
        }
    }

    #[test]
    fn validate_request_rejects_empty_prompt() {
        let dir = tempdir().expect("tempdir");
        let mut request = valid_request(dir.path().to_string_lossy().into_owned());
        request.prompt = "   ".to_string();

        let error = validate_request(&request).expect_err("empty prompt should fail");
        assert_eq!(error, "prompt cannot be empty");
    }

    #[test]
    fn validate_request_rejects_missing_workspace() {
        let request = valid_request("c:/definitely/missing".to_string());
        let error = validate_request(&request).expect_err("missing workspace should fail");
        assert_eq!(error, "workspace does not exist");
    }

    #[test]
    fn persist_runner_update_records_events_and_final_state() {
        let (_dir, store) = make_store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Launching,
                status: "Launching".to_string(),
                settings: RunSettings::default(),
            })
            .expect("create session");

        persist_runner_update(
            &store,
            "run-1",
            &Event::new("app", EventKind::Status, "turn started"),
            false,
        )
        .expect("persist running update");
        persist_runner_update(
            &store,
            "run-1",
            &Event::new(
                "session",
                EventKind::Status,
                "codex run completed successfully",
            ),
            true,
        )
        .expect("persist finished update");

        let session = store
            .get_session("run-1")
            .expect("get session")
            .expect("session exists");
        let events = store.list_events("run-1", None).expect("events");

        assert_eq!(events.len(), 2);
        assert_eq!(session.total_events, 2);
        assert_eq!(session.lifecycle, SessionLifecycle::Completed);
        assert_eq!(session.status, "Completed");
    }

    #[test]
    fn extracts_codex_session_id_from_thread_started_event() {
        let event = Event::new(
            "thread",
            EventKind::Status,
            "thread started: 019ccdee-5bdb-7602-95df-d6edbfd0083c",
        );

        assert_eq!(
            extract_codex_session_id(&event).as_deref(),
            Some("019ccdee-5bdb-7602-95df-d6edbfd0083c")
        );
    }

    #[test]
    fn derives_next_session_seed_from_highest_persisted_run_id() {
        let (_dir, store) = make_store();
        store
            .create_session(&CreateSessionInput {
                id: "run-2".to_string(),
                thread_id: "thread-2".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Completed,
                status: "Completed".to_string(),
                settings: RunSettings::default(),
            })
            .expect("create session 2");
        store
            .create_session(&CreateSessionInput {
                id: "run-8".to_string(),
                thread_id: "thread-8".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Completed,
                status: "Completed".to_string(),
                settings: RunSettings::default(),
            })
            .expect("create session 8");
        store
            .create_session(&CreateSessionInput {
                id: "imported-session".to_string(),
                thread_id: "imported-session".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Completed,
                status: "Completed".to_string(),
                settings: RunSettings::default(),
            })
            .expect("create non-run session");

        let seed = next_session_seed(&store).expect("derive next session seed");
        assert_eq!(seed, 9);
    }

    #[test]
    fn reconciles_orphaned_active_sessions_on_startup() {
        let (_dir, store) = make_store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Running,
                status: "Running".to_string(),
                settings: RunSettings::default(),
            })
            .expect("create running session");
        store
            .create_session(&CreateSessionInput {
                id: "run-2".to_string(),
                thread_id: "thread-2".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Cancelling,
                status: "Cancelling".to_string(),
                settings: RunSettings::default(),
            })
            .expect("create cancelling session");

        reconcile_orphaned_sessions(&store).expect("reconcile sessions");

        let run_1 = store
            .get_session("run-1")
            .expect("load run-1")
            .expect("run-1 exists");
        let run_2 = store
            .get_session("run-2")
            .expect("load run-2")
            .expect("run-2 exists");

        assert_eq!(run_1.lifecycle, SessionLifecycle::Failed);
        assert_eq!(run_1.status, "Stopped before completion");
        assert_eq!(run_2.lifecycle, SessionLifecycle::Cancelled);
        assert_eq!(run_2.status, "Cancelled");
    }

    #[test]
    fn cancel_can_reconcile_orphaned_running_session() {
        let (_dir, store) = make_store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Running,
                status: "Running".to_string(),
                settings: RunSettings::default(),
            })
            .expect("create running session");

        let repaired = reconcile_cancelled_orphaned_session(&store, "thread-1")
            .expect("reconcile cancelled session")
            .expect("session exists");

        assert_eq!(repaired.lifecycle, SessionLifecycle::Cancelled);
        assert_eq!(repaired.status, "Cancelled");
        assert_eq!(
            repaired.last_message.as_deref(),
            Some("session cancelled after losing the active process")
        );
    }

    #[test]
    fn reuses_codex_session_only_when_workspace_matches() {
        let session = StoredSession {
            id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            attempt_no: 1,
            title: "Prompt".to_string(),
            prompt: "prompt".to_string(),
            workspace: r"\\?\C:\Users\joshs\Projects\repo-a".to_string(),
            codex_session_id: Some("019ccdee-5bdb-7602-95df-d6edbfd0083c".to_string()),
            resume_ready: true,
            lifecycle: SessionLifecycle::Completed,
            status: "Completed".to_string(),
            created_at: 1,
            updated_at: 2,
            last_event_at: Some(2),
            last_message: Some("done".to_string()),
            total_events: 1,
            command_count: 0,
            warning_count: 0,
            error_count: 0,
            settings: RunSettings::default(),
        };

        assert_eq!(
            resume_codex_session_id(
                &session,
                r"C:\Users\joshs\Projects\repo-a",
                &RunSettings::default()
            )
            .as_deref(),
            Some("019ccdee-5bdb-7602-95df-d6edbfd0083c")
        );
        assert_eq!(
            resume_codex_session_id(
                &session,
                r"C:\Users\joshs\Projects\repo-b",
                &RunSettings::default()
            ),
            None
        );
    }

    #[test]
    fn does_not_resume_codex_session_for_first_run_state() {
        let session = StoredSession {
            id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            attempt_no: 1,
            title: "Prompt".to_string(),
            prompt: "prompt".to_string(),
            workspace: r"C:\Users\joshs\Projects\repo-a".to_string(),
            codex_session_id: Some("019ccdee-5bdb-7602-95df-d6edbfd0083c".to_string()),
            resume_ready: false,
            lifecycle: SessionLifecycle::Launching,
            status: "Launching".to_string(),
            created_at: 1,
            updated_at: 2,
            last_event_at: None,
            last_message: None,
            total_events: 0,
            command_count: 0,
            warning_count: 0,
            error_count: 0,
            settings: RunSettings::default(),
        };

        assert_eq!(
            resume_codex_session_id(
                &session,
                r"C:\Users\joshs\Projects\repo-a",
                &RunSettings::default()
            ),
            None
        );
    }

    #[test]
    fn does_not_resume_when_settings_change() {
        let session = StoredSession {
            id: "run-1".to_string(),
            thread_id: "thread-1".to_string(),
            attempt_no: 2,
            title: "Prompt".to_string(),
            prompt: "prompt".to_string(),
            workspace: r"C:\Users\joshs\Projects\repo-a".to_string(),
            codex_session_id: Some("019ccdee-5bdb-7602-95df-d6edbfd0083c".to_string()),
            resume_ready: true,
            lifecycle: SessionLifecycle::Completed,
            status: "Completed".to_string(),
            created_at: 1,
            updated_at: 2,
            last_event_at: Some(2),
            last_message: Some("done".to_string()),
            total_events: 2,
            command_count: 0,
            warning_count: 0,
            error_count: 0,
            settings: RunSettings::default(),
        };

        assert_eq!(
            resume_codex_session_id(
                &session,
                r"C:\Users\joshs\Projects\repo-a",
                &RunSettings {
                    model: String::new(),
                    sandbox: "danger-full-access".to_string(),
                    approval: "never".to_string(),
                    bypass_approvals_and_sandbox: false,
                }
            ),
            None
        );
        assert_eq!(
            resume_codex_session_id(
                &session,
                r"C:\Users\joshs\Projects\repo-a",
                &RunSettings {
                    model: String::new(),
                    sandbox: "workspace-write".to_string(),
                    approval: "on-request".to_string(),
                    bypass_approvals_and_sandbox: false,
                }
            ),
            None
        );
    }
}
