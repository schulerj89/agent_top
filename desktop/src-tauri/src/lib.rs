mod storage;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use agent_top_core::{
    start_codex_run, Event, EventKind, ManagedRun, RunController, RunRequest, RunSettings,
    SessionLifecycle,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::storage::{
    default_db_path, CreateSessionInput, SessionStore, SessionUpdate, StoredEvent, StoredSession,
};

struct AppState {
    default_workspace: String,
    store: SessionStore,
    next_session_id: AtomicU64,
    active_runs: Mutex<HashMap<String, RunController>>,
}

#[derive(Serialize)]
struct BootstrapPayload {
    workspace: String,
    settings: SettingsPayload,
    sessions: Vec<SessionListItem>,
}

#[derive(Clone, Deserialize, Serialize)]
struct SettingsPayload {
    model: String,
    sandbox: String,
    approval: String,
}

#[derive(Clone, Deserialize)]
struct RunRequestPayload {
    prompt: String,
    workspace: String,
    settings: SettingsPayload,
}

#[derive(Deserialize)]
struct CancelRunRequest {
    session_id: String,
}

#[derive(Deserialize)]
struct SessionLookupRequest {
    session_id: String,
    limit: Option<usize>,
}

#[derive(Clone, Serialize)]
struct SessionListItem {
    session_id: String,
    title: String,
    prompt: String,
    workspace: String,
    lifecycle: String,
    status: String,
    updated_at: String,
    last_event_at: Option<String>,
    last_message: Option<String>,
    total_events: usize,
    command_count: usize,
    warning_count: usize,
    error_count: usize,
    settings: SettingsPayload,
}

#[derive(Clone, Serialize)]
struct SessionEventPayload {
    id: i64,
    session_id: String,
    timestamp: String,
    kind: String,
    message: String,
    payload_json: Option<String>,
    sequence_no: i64,
}

#[derive(Clone, Serialize)]
struct EventPayload {
    session_id: String,
    timestamp: String,
    kind: String,
    message: String,
    finished: bool,
    lifecycle: String,
}

#[derive(Serialize)]
struct StartRunResponse {
    session_id: String,
}

#[tauri::command]
fn bootstrap(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    let sessions = state
        .store
        .list_sessions(Some(50))?
        .into_iter()
        .map(SessionListItem::from)
        .collect();
    Ok(BootstrapPayload {
        workspace: state.default_workspace.clone(),
        settings: SettingsPayload::default(),
        sessions,
    })
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionListItem>, String> {
    state
        .store
        .list_sessions(Some(200))?
        .into_iter()
        .map(SessionListItem::from)
        .collect::<Vec<_>>()
        .pipe(Ok)
}

#[tauri::command]
fn get_session(
    state: State<'_, AppState>,
    request: SessionLookupRequest,
) -> Result<Option<SessionListItem>, String> {
    state
        .store
        .get_session(&request.session_id)?
        .map(SessionListItem::from)
        .pipe(Ok)
}

#[tauri::command]
fn get_session_events(
    state: State<'_, AppState>,
    request: SessionLookupRequest,
) -> Result<Vec<SessionEventPayload>, String> {
    state
        .store
        .list_events(&request.session_id, request.limit)?
        .into_iter()
        .map(SessionEventPayload::from)
        .collect::<Vec<_>>()
        .pipe(Ok)
}

#[tauri::command]
fn pick_workspace(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|path| path.to_string())
}

#[tauri::command]
fn start_run(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RunRequestPayload,
) -> Result<StartRunResponse, String> {
    validate_request(&request)?;
    let session_id = format!(
        "run-{}",
        state.next_session_id.fetch_add(1, Ordering::Relaxed)
    );
    let settings = RunSettings {
        model: request.settings.model.clone(),
        sandbox: request.settings.sandbox.clone(),
        approval: request.settings.approval.clone(),
    };

    state.store.create_session(&CreateSessionInput {
        id: session_id.clone(),
        prompt: request.prompt.clone(),
        workspace: request.workspace.clone(),
        lifecycle: SessionLifecycle::Launching,
        status: "Launching".to_string(),
        settings: settings.clone(),
    })?;

    let managed = start_codex_run(RunRequest {
        session_id: session_id.clone(),
        prompt: request.prompt,
        workspace: request.workspace,
        settings,
    });

    register_run(state.inner(), &session_id, managed.controller.clone())?;
    forward_events(app, state.store.clone(), session_id.clone(), managed);
    Ok(StartRunResponse { session_id })
}

#[tauri::command]
fn cancel_run(state: State<'_, AppState>, request: CancelRunRequest) -> Result<(), String> {
    let controller = {
        let guard = state
            .active_runs
            .lock()
            .map_err(|_| "active run state is unavailable".to_string())?;
        guard.get(&request.session_id).cloned()
    }
    .ok_or_else(|| "session is not running".to_string())?;

    controller.cancel().map(|_| ())
}

#[tauri::command]
fn retry_run(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CancelRunRequest,
) -> Result<StartRunResponse, String> {
    let record = state
        .store
        .get_session(&request.session_id)?
        .ok_or_else(|| "session history entry not found".to_string())?;

    let session_id = format!(
        "run-{}",
        state.next_session_id.fetch_add(1, Ordering::Relaxed)
    );
    let settings = record.settings.clone();
    state.store.create_session(&CreateSessionInput {
        id: session_id.clone(),
        prompt: record.prompt.clone(),
        workspace: record.workspace.clone(),
        lifecycle: SessionLifecycle::Launching,
        status: "Launching".to_string(),
        settings: settings.clone(),
    })?;

    let managed = start_codex_run(RunRequest {
        session_id: session_id.clone(),
        prompt: record.prompt,
        workspace: record.workspace,
        settings,
    });

    register_run(state.inner(), &session_id, managed.controller.clone())?;
    forward_events(app, state.store.clone(), session_id.clone(), managed);
    Ok(StartRunResponse { session_id })
}

impl Default for SettingsPayload {
    fn default() -> Self {
        Self {
            model: String::new(),
            sandbox: "workspace-write".to_string(),
            approval: "never".to_string(),
        }
    }
}

impl EventPayload {
    fn from_event(session_id: String, event: Event, finished: bool) -> Self {
        let lifecycle = lifecycle_for_event(&event, finished);

        Self {
            session_id,
            timestamp: event.timestamp,
            kind: kind_label(event.kind).to_string(),
            message: event.message,
            finished,
            lifecycle: lifecycle_label(lifecycle).to_string(),
        }
    }
}

impl From<StoredSession> for SessionListItem {
    fn from(value: StoredSession) -> Self {
        Self {
            session_id: value.id,
            title: value.title,
            prompt: value.prompt,
            workspace: value.workspace,
            lifecycle: lifecycle_label(value.lifecycle).to_string(),
            status: value.status,
            updated_at: value.updated_at.to_string(),
            last_event_at: value.last_event_at.map(|value| value.to_string()),
            last_message: value.last_message,
            total_events: value.total_events,
            command_count: value.command_count,
            warning_count: value.warning_count,
            error_count: value.error_count,
            settings: SettingsPayload {
                model: value.settings.model,
                sandbox: value.settings.sandbox,
                approval: value.settings.approval,
            },
        }
    }
}

impl From<StoredEvent> for SessionEventPayload {
    fn from(value: StoredEvent) -> Self {
        Self {
            id: value.id,
            session_id: value.session_id,
            timestamp: value.ts.to_string(),
            kind: kind_label(value.kind).to_string(),
            message: value.message,
            payload_json: value.payload_json,
            sequence_no: value.sequence_no,
        }
    }
}

fn validate_request(request: &RunRequestPayload) -> Result<(), String> {
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

fn register_run(
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

fn forward_events(app: AppHandle, store: SessionStore, session_id: String, managed: ManagedRun) {
    std::thread::spawn(move || {
        while let Ok(update) = managed.receiver.recv() {
            let event = update.event.clone();
            let _ = persist_runner_update(&store, &session_id, &event, update.finished);
            let _ = app.emit(
                "agent-event",
                EventPayload::from_event(session_id.clone(), event, update.finished),
            );

            if update.finished {
                let state: State<'_, AppState> = app.state();
                if let Ok(mut guard) = state.active_runs.lock() {
                    guard.remove(&session_id);
                }
                break;
            }
        }
    });
}

fn detect_workspace() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .unwrap_or(manifest_dir);

    candidate
        .canonicalize()
        .unwrap_or(candidate)
        .to_string_lossy()
        .into_owned()
}

fn persist_runner_update(
    store: &SessionStore,
    session_id: &str,
    event: &Event,
    finished: bool,
) -> Result<(), String> {
    let payload_json = serde_json::to_string(event).ok();
    store.append_event(session_id, event, payload_json.as_deref())?;

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
    )
}

fn lifecycle_for_event(event: &Event, finished: bool) -> SessionLifecycle {
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

fn status_for_finished_lifecycle(lifecycle: SessionLifecycle) -> &'static str {
    match lifecycle {
        SessionLifecycle::Launching => "Launching",
        SessionLifecycle::Running => "Running",
        SessionLifecycle::Cancelling => "Cancelling",
        SessionLifecycle::Cancelled => "Cancelled",
        SessionLifecycle::Completed => "Completed",
        SessionLifecycle::Failed => "Failed",
    }
}

fn lifecycle_label(lifecycle: SessionLifecycle) -> &'static str {
    match lifecycle {
        SessionLifecycle::Launching => "launching",
        SessionLifecycle::Running => "running",
        SessionLifecycle::Cancelling => "cancelling",
        SessionLifecycle::Cancelled => "cancelled",
        SessionLifecycle::Completed => "completed",
        SessionLifecycle::Failed => "failed",
    }
}

fn kind_label(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Status => "status",
        EventKind::Command => "command",
        EventKind::File => "file",
        EventKind::Warning => "warning",
        EventKind::Error => "error",
        EventKind::Note => "note",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let database_path = default_db_path();
            let store = SessionStore::new(database_path.clone());
            store.init().map_err(io::Error::other)?;

            let state = AppState {
                default_workspace: detect_workspace(),
                store,
                next_session_id: AtomicU64::new(1),
                active_runs: Mutex::new(HashMap::new()),
            };

            app.manage(state);

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.handle().plugin(tauri_plugin_dialog::init())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            list_sessions,
            get_session,
            get_session_events,
            pick_workspace,
            start_run,
            cancel_run,
            retry_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn session_summary_mapping_preserves_counts() {
        let item = SessionListItem::from(StoredSession {
            id: "run-1".to_string(),
            title: "Prompt".to_string(),
            prompt: "prompt".to_string(),
            workspace: "c:/repo".to_string(),
            lifecycle: SessionLifecycle::Running,
            status: "Running".to_string(),
            created_at: 1,
            updated_at: 2,
            last_event_at: Some(2),
            last_message: Some("latest".to_string()),
            total_events: 4,
            command_count: 1,
            warning_count: 2,
            error_count: 1,
            settings: RunSettings::default(),
        });

        assert_eq!(item.session_id, "run-1");
        assert_eq!(item.lifecycle, "running");
        assert_eq!(item.total_events, 4);
        assert_eq!(item.error_count, 1);
    }
}
