use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_top_core::{spawn_codex_run, Event, EventKind, RunSettings};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
  default_workspace: String,
  running: Arc<Mutex<bool>>,
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
  timestamp: String,
  kind: String,
  message: String,
  finished: bool,
}

#[tauri::command]
fn bootstrap(state: State<'_, AppState>) -> BootstrapPayload {
  BootstrapPayload {
    workspace: state.default_workspace.clone(),
    settings: SettingsPayload::default(),
  }
}

#[tauri::command]
fn start_run(
  app: AppHandle,
  state: State<'_, AppState>,
  request: RunRequest,
) -> Result<(), String> {
  if request.prompt.trim().is_empty() {
    return Err("prompt cannot be empty".to_string());
  }

  let mut running = state
    .running
    .lock()
    .map_err(|_| "failed to lock run state".to_string())?;

  if *running {
    return Err("a run is already active".to_string());
  }

  *running = true;
  drop(running);

  let receiver = spawn_codex_run(
    request.prompt,
    request.workspace,
    RunSettings {
      model: request.settings.model,
      sandbox: request.settings.sandbox,
      approval: request.settings.approval,
    },
  );

  let running = state.running.clone();
  std::thread::spawn(move || {
    while let Ok(update) = receiver.recv() {
      let _ = app.emit("agent-event", EventPayload::from_event(update.event, update.finished));
      if update.finished {
        if let Ok(mut flag) = running.lock() {
          *flag = false;
        }
        break;
      }
    }
  });

  Ok(())
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
  fn from_event(event: Event, finished: bool) -> Self {
    Self {
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
        running: Arc::new(Mutex::new(false)),
      });

      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![bootstrap, start_run])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
