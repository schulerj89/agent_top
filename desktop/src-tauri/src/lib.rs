use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use agent_top_core::{
    load_session_history, next_session_id, start_codex_run, Event, EventKind, ManagedRun,
    RunController, RunRequest, RunSettings, SessionLifecycle, SessionRecord,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

struct AppState {
    default_workspace: String,
    next_session_id: AtomicU64,
    active_runs: Mutex<HashMap<String, RunController>>,
}

#[derive(Serialize)]
struct BootstrapPayload {
    workspace: String,
    settings: SettingsPayload,
    sessions: Vec<SessionRecord>,
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
    let sessions = load_session_history().map_err(|error| error.to_string())?;
    Ok(BootstrapPayload {
        workspace: state.default_workspace.clone(),
        settings: SettingsPayload::default(),
        sessions,
    })
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

    let managed = start_codex_run(RunRequest {
        session_id: session_id.clone(),
        prompt: request.prompt,
        workspace: request.workspace,
        settings: RunSettings {
            model: request.settings.model,
            sandbox: request.settings.sandbox,
            approval: request.settings.approval,
        },
    });

    register_run(state.inner(), &session_id, managed.controller.clone())?;
    forward_events(app, session_id.clone(), managed);
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
    let record = load_session_history()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|item| item.session_id == request.session_id)
        .ok_or_else(|| "session history entry not found".to_string())?;

    let session_id = format!(
        "run-{}",
        state.next_session_id.fetch_add(1, Ordering::Relaxed)
    );
    let managed = start_codex_run(RunRequest {
        session_id: session_id.clone(),
        prompt: record.prompt,
        workspace: record.workspace,
        settings: record.settings,
    });

    register_run(state.inner(), &session_id, managed.controller.clone())?;
    forward_events(app, session_id.clone(), managed);
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
        let lifecycle = if finished {
            match event.kind {
                EventKind::Status => SessionLifecycle::Completed,
                EventKind::Warning if event.message.contains("cancelled") => {
                    SessionLifecycle::Cancelled
                }
                _ => SessionLifecycle::Failed,
            }
        } else {
            SessionLifecycle::Running
        };

        Self {
            session_id,
            timestamp: event.timestamp,
            kind: match event.kind {
                EventKind::Status => "status",
                EventKind::Command => "command",
                EventKind::File => "file",
                EventKind::Warning => "warning",
                EventKind::Error => "error",
                EventKind::Note => "note",
            }
            .to_string(),
            message: event.message,
            finished,
            lifecycle: format!("{lifecycle:?}").to_ascii_lowercase(),
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

fn forward_events(app: AppHandle, session_id: String, managed: ManagedRun) {
    std::thread::spawn(move || {
        while let Ok(update) = managed.receiver.recv() {
            let _ = app.emit(
                "agent-event",
                EventPayload::from_event(session_id.clone(), update.event, update.finished),
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = AppState {
                default_workspace: detect_workspace(),
                next_session_id: AtomicU64::new(1),
                active_runs: Mutex::new(HashMap::new()),
            };

            let _ = next_session_id();
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
            pick_workspace,
            start_run,
            cancel_run,
            retry_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
