use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

use agent_top_core::{RunController, RunSettings};
use serde::{Deserialize, Serialize};

use crate::storage::SessionStore;
use crate::Pipe;

pub struct AppState {
    pub default_workspace: String,
    pub store: SessionStore,
    pub next_session_id: AtomicU64,
    pub active_runs: Mutex<HashMap<String, RunController>>,
}

#[derive(Serialize)]
pub struct BootstrapPayload {
    pub workspace: String,
    pub settings: SettingsPayload,
    pub sessions: Vec<crate::commands::SessionListItem>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct SettingsPayload {
    pub model: String,
    pub sandbox: String,
    pub approval: String,
    pub bypass_approvals_and_sandbox: bool,
}

#[derive(Clone, Deserialize)]
pub struct RunRequestPayload {
    pub prompt: String,
    pub workspace: String,
    pub settings: SettingsPayload,
}

#[derive(Deserialize)]
pub struct CancelRunRequest {
    pub session_id: String,
}

#[derive(Serialize)]
pub struct DeleteSessionResponse {
    pub deleted: bool,
}

#[derive(Serialize)]
pub struct CancelRunResponse {
    pub session: Option<crate::commands::SessionListItem>,
}

#[derive(Deserialize)]
pub struct SessionLookupRequest {
    pub session_id: String,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct StartRunResponse {
    pub session_id: String,
}

impl Default for SettingsPayload {
    fn default() -> Self {
        Self {
            model: String::new(),
            sandbox: "workspace-write".to_string(),
            approval: "never".to_string(),
            bypass_approvals_and_sandbox: false,
        }
    }
}

pub fn settings_payload_to_run(settings: &SettingsPayload) -> RunSettings {
    RunSettings {
        model: settings.model.clone(),
        sandbox: settings.sandbox.clone(),
        approval: settings.approval.clone(),
        bypass_approvals_and_sandbox: settings.bypass_approvals_and_sandbox,
    }
}

pub fn detect_workspace() -> String {
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
        .pipe(|path| normalize_workspace_display(&path))
}

pub fn normalize_workspace_display(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{stripped}");
    }

    path.strip_prefix(r"\\?\")
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_windows_workspace_prefixes() {
        assert_eq!(
            normalize_workspace_display(r"\\?\C:\Users\joshs\Projects\agent_top"),
            r"C:\Users\joshs\Projects\agent_top"
        );
        assert_eq!(
            normalize_workspace_display(r"\\?\UNC\server\share\repo"),
            r"\\server\share\repo"
        );
        assert_eq!(
            normalize_workspace_display(r"C:\Users\joshs\Projects\agent_top"),
            r"C:\Users\joshs\Projects\agent_top"
        );
    }

    #[test]
    fn maps_settings_payload_to_run_settings() {
        let mapped = settings_payload_to_run(&SettingsPayload {
            model: "gpt-5".to_string(),
            sandbox: "danger-full-access".to_string(),
            approval: "never".to_string(),
            bypass_approvals_and_sandbox: true,
        });

        assert_eq!(mapped.model, "gpt-5");
        assert_eq!(mapped.sandbox, "danger-full-access");
        assert_eq!(mapped.approval, "never");
        assert!(mapped.bypass_approvals_and_sandbox);
    }
}
