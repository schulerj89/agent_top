use std::sync::atomic::Ordering;

use agent_top_core::{Event, EventKind, SessionLifecycle};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::{
    settings_payload_to_run, AppState, BootstrapPayload, CancelRunRequest, CancelRunResponse,
    DeleteSessionResponse, RunRequestPayload, SessionLookupRequest, SettingsPayload,
    StartRunResponse,
};
use crate::runtime::{
    has_active_run, launch_run, reconcile_cancelled_orphaned_session, resume_codex_session_id,
    validate_request,
};
use crate::storage::{CreateSessionInput, SessionRunUpdate, StoredEvent, StoredSession};
use crate::Pipe;

#[derive(Clone, Serialize)]
pub struct SessionListItem {
    pub session_id: String,
    pub title: String,
    pub prompt: String,
    pub workspace: String,
    pub codex_session_id: Option<String>,
    pub lifecycle: String,
    pub status: String,
    pub updated_at: String,
    pub last_event_at: Option<String>,
    pub last_message: Option<String>,
    pub total_events: usize,
    pub command_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
    pub settings: SettingsPayload,
}

#[derive(Clone, Serialize)]
pub struct SessionEventPayload {
    pub id: i64,
    pub session_id: String,
    pub timestamp: String,
    pub kind: String,
    pub message: String,
    pub payload_json: Option<String>,
    pub sequence_no: i64,
}

#[derive(Clone, Serialize)]
pub struct EventPayload {
    pub session_id: String,
    pub timestamp: String,
    pub kind: String,
    pub message: String,
    pub finished: bool,
    pub lifecycle: String,
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
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
pub fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionListItem>, String> {
    state
        .store
        .list_sessions(Some(200))?
        .into_iter()
        .map(SessionListItem::from)
        .collect::<Vec<_>>()
        .pipe(Ok)
}

#[tauri::command]
pub fn get_session(
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
pub fn get_session_events(
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
pub fn pick_workspace(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|path| crate::app_state::normalize_workspace_display(&path.to_string()))
}

#[tauri::command]
pub fn start_run(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RunRequestPayload,
) -> Result<StartRunResponse, String> {
    validate_request(&request)?;
    let workspace = crate::app_state::normalize_workspace_display(&request.workspace);
    let session_id = format!(
        "run-{}",
        state.next_session_id.fetch_add(1, Ordering::Relaxed)
    );
    let settings = settings_payload_to_run(&request.settings);

    state.store.create_session(&CreateSessionInput {
        id: session_id.clone(),
        prompt: request.prompt.clone(),
        workspace: workspace.clone(),
        lifecycle: SessionLifecycle::Launching,
        status: "Launching".to_string(),
        settings: settings.clone(),
    })?;

    launch_run(
        app,
        state.inner(),
        session_id.clone(),
        request.prompt,
        workspace,
        settings,
        None,
    )?;
    Ok(StartRunResponse { session_id })
}

#[tauri::command]
pub fn cancel_run(
    state: State<'_, AppState>,
    request: CancelRunRequest,
) -> Result<CancelRunResponse, String> {
    let controller = {
        let guard = state
            .active_runs
            .lock()
            .map_err(|_| "active run state is unavailable".to_string())?;
        guard.get(&request.session_id).cloned()
    };

    if let Some(controller) = controller {
        controller.cancel().map(|_| CancelRunResponse { session: None })
    } else {
        let repaired = reconcile_cancelled_orphaned_session(&state.store, &request.session_id)?;
        Ok(CancelRunResponse {
            session: repaired.map(SessionListItem::from),
        })
    }
}

#[tauri::command]
pub fn retry_run(
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

    launch_run(
        app,
        state.inner(),
        session_id.clone(),
        record.prompt,
        record.workspace,
        settings,
        None,
    )?;
    Ok(StartRunResponse { session_id })
}

#[tauri::command]
pub fn continue_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SessionLookupRequest,
    run: RunRequestPayload,
) -> Result<StartRunResponse, String> {
    validate_request(&run)?;
    if has_active_run(state.inner(), &request.session_id)? {
        return Err("session is already running".to_string());
    }

    let workspace = crate::app_state::normalize_workspace_display(&run.workspace);
    let settings = settings_payload_to_run(&run.settings);
    let existing = state
        .store
        .get_session(&request.session_id)?
        .ok_or_else(|| "session history entry not found".to_string())?;
    let resume_codex_session_id = resume_codex_session_id(&existing, &workspace, &settings);
    let updated = state.store.prepare_session_run(
        &request.session_id,
        &SessionRunUpdate {
            prompt: run.prompt.clone(),
            workspace: workspace.clone(),
            codex_session_id: resume_codex_session_id.clone(),
            lifecycle: SessionLifecycle::Launching,
            status: "Launching".to_string(),
            settings: settings.clone(),
        },
    )?;

    if !updated {
        return Err("session history entry not found".to_string());
    }

    launch_run(
        app,
        state.inner(),
        request.session_id.clone(),
        run.prompt,
        workspace,
        settings,
        resume_codex_session_id,
    )?;
    Ok(StartRunResponse {
        session_id: request.session_id,
    })
}

#[tauri::command]
pub fn delete_session(
    state: State<'_, AppState>,
    request: CancelRunRequest,
) -> Result<DeleteSessionResponse, String> {
    if has_active_run(state.inner(), &request.session_id)? {
        return Err("session is still running".to_string());
    }

    Ok(DeleteSessionResponse {
        deleted: state.store.delete_session(&request.session_id)?,
    })
}

impl EventPayload {
    pub fn from_event(session_id: String, event: Event, finished: bool) -> Self {
        let lifecycle = crate::runtime::lifecycle_for_event(&event, finished);

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
            workspace: crate::app_state::normalize_workspace_display(&value.workspace),
            codex_session_id: value.codex_session_id,
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
                bypass_approvals_and_sandbox: value.settings.bypass_approvals_and_sandbox,
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

pub fn lifecycle_label(lifecycle: SessionLifecycle) -> &'static str {
    match lifecycle {
        SessionLifecycle::Launching => "launching",
        SessionLifecycle::Running => "running",
        SessionLifecycle::Cancelling => "cancelling",
        SessionLifecycle::Cancelled => "cancelled",
        SessionLifecycle::Completed => "completed",
        SessionLifecycle::Failed => "failed",
    }
}

pub fn kind_label(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Status => "status",
        EventKind::Command => "command",
        EventKind::File => "file",
        EventKind::Warning => "warning",
        EventKind::Error => "error",
        EventKind::Note => "note",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_top_core::RunSettings;

    #[test]
    fn session_summary_mapping_preserves_counts() {
        let item = SessionListItem::from(StoredSession {
            id: "run-1".to_string(),
            title: "Prompt".to_string(),
            prompt: "prompt".to_string(),
            workspace: "c:/repo".to_string(),
            codex_session_id: Some("019ccdee-5bdb-7602-95df-d6edbfd0083c".to_string()),
            resume_ready: false,
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
