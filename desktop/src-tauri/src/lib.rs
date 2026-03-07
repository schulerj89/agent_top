use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_top_core::{spawn_codex_run, Event, EventKind, RunSettings};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

struct AppState {
  default_workspace: String,
  next_session_id: AtomicU64,
}

#[derive(Serialize)]
struct BootstrapPayload {
  workspace: String,
  settings: SettingsPayload,
}

#[derive(Clone, Deserialize, Serialize)]
struct SettingsPayload {
  model: String,
  sandbox: String,
  approval: String,
}

#[derive(Deserialize)]
struct RunRequest {
  prompt: String,
  workspace: String,
  settings: SettingsPayload,
}

#[derive(Clone, Serialize)]
struct EventPayload {
  session_id: String,
  timestamp: String,
  kind: String,
  message: String,
  finished: bool,
}

#[derive(Serialize)]
struct StartRunResponse {
  session_id: String,
}

#[tauri::command]
fn bootstrap(state: State<'_, AppState>) -> BootstrapPayload {
  BootstrapPayload {
    workspace: state.default_workspace.clone(),
    settings: SettingsPayload::default(),
  }
}

#[tauri::command]
fn pick_workspace(app: AppHandle) -> Option<String> {
  app
    .dialog()
    .file()
    .blocking_pick_folder()
    .map(|path| path.to_string())
}

#[tauri::command]
fn start_run(
  app: AppHandle,
  state: State<'_, AppState>,
  request: RunRequest,
) -> Result<StartRunResponse, String> {
  if request.prompt.trim().is_empty() {
    return Err("prompt cannot be empty".to_string());
  }

  let session_id = format!("run-{}", state.next_session_id.fetch_add(1, Ordering::Relaxed));

  let receiver = spawn_codex_run(
    request.prompt,
    request.workspace,
    RunSettings {
      model: request.settings.model,
      sandbox: request.settings.sandbox,
      approval: request.settings.approval,
    },
  );

  let emitted_session_id = session_id.clone();
  std::thread::spawn(move || {
    while let Ok(update) = receiver.recv() {
      let _ = app.emit(
        "agent-event",
        EventPayload::from_event(emitted_session_id.clone(), update.event, update.finished),
      );
      if update.finished {
        break;
      }
    }
  });

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
    }
  }
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
      app.manage(AppState {
        default_workspace: detect_workspace(),
        next_session_id: AtomicU64::new(1),
      });

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
    .invoke_handler(tauri::generate_handler![bootstrap, pick_workspace, start_run])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
