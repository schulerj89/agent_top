use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Status,
    Command,
    File,
    Warning,
    Error,
    Note,
}

impl EventKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "status" => Some(Self::Status),
            "command" => Some(Self::Command),
            "file" => Some(Self::File),
            "warning" => Some(Self::Warning),
            "error" => Some(Self::Error),
            "note" => Some(Self::Note),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandDetails {
    pub command: String,
    pub stage: String,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<u64>,
    pub aggregated_output: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileDetails {
    pub path: String,
    pub group: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventDetails {
    pub command: Option<CommandDetails>,
    pub file: Option<FileDetails>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: String,
    pub kind: EventKind,
    pub message: String,
    pub details: Option<EventDetails>,
}

impl Event {
    pub fn new(timestamp: impl Into<String>, kind: EventKind, message: impl Into<String>) -> Self {
        Self {
            timestamp: timestamp.into(),
            kind,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: EventDetails) -> Self {
        self.details = Some(details);
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandAnalytics {
    pub command: String,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Analytics {
    pub command_runs: Vec<CommandAnalytics>,
    pub exit_status_counts: BTreeMap<String, usize>,
    pub file_groups: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub source: String,
    pub current_status: Option<String>,
    pub commands: usize,
    pub warnings: usize,
    pub errors: usize,
    pub files_touched: BTreeSet<String>,
    pub recent_events: Vec<Event>,
    pub total_events: usize,
    pub all_events: Vec<Event>,
    pub analytics: Analytics,
}

impl Summary {
    pub fn with_source(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            ..Self::default()
        }
    }

    pub fn record(&mut self, event: Event) {
        self.total_events += 1;

        match event.kind {
            EventKind::Status => {
                self.current_status = Some(event.message.clone());
            }
            EventKind::Command => {
                self.commands += 1;
            }
            EventKind::File => {
                self.files_touched.insert(event.message.clone());
            }
            EventKind::Warning => {
                self.warnings += 1;
            }
            EventKind::Error => {
                self.errors += 1;
            }
            EventKind::Note => {}
        }

        if let Some(details) = &event.details {
            if let Some(command) = &details.command {
                if command.stage == "completed" {
                    self.analytics.command_runs.push(CommandAnalytics {
                        command: command.command.clone(),
                        exit_code: command.exit_code,
                        duration_ms: command.duration_ms,
                    });

                    let key = command
                        .exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    *self.analytics.exit_status_counts.entry(key).or_default() += 1;
                }
            }

            if let Some(file) = &details.file {
                self.files_touched.insert(file.path.clone());
                *self
                    .analytics
                    .file_groups
                    .entry(file.group.clone())
                    .or_default() += 1;
            }
        }

        self.recent_events.push(event.clone());
        if self.recent_events.len() > 8 {
            self.recent_events.remove(0);
        }

        self.all_events.push(event);
        if self.all_events.len() > 200 {
            self.all_events.drain(0..self.all_events.len() - 200);
        }
    }

    pub fn filtered_events(&self, query: &str, kind: Option<EventKind>) -> Vec<&Event> {
        let query = query.trim().to_ascii_lowercase();
        self.all_events
            .iter()
            .filter(|event| kind.map(|value| event.kind == value).unwrap_or(true))
            .filter(|event| {
                query.is_empty()
                    || event.message.to_ascii_lowercase().contains(&query)
                    || event.timestamp.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunSettings {
    pub model: String,
    pub sandbox: String,
    pub approval: String,
    pub bypass_approvals_and_sandbox: bool,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            model: String::new(),
            sandbox: "workspace-write".to_string(),
            approval: "never".to_string(),
            bypass_approvals_and_sandbox: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunRequest {
    pub session_id: String,
    pub prompt: String,
    pub workspace: String,
    pub settings: RunSettings,
    pub codex_session_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLifecycle {
    Launching,
    Running,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

pub struct RunnerUpdate {
    pub event: Event,
    pub finished: bool,
}

impl RunnerUpdate {
    pub fn event(event: Event) -> Self {
        Self {
            event,
            finished: false,
        }
    }

    pub fn finished(event: Event) -> Self {
        Self {
            event,
            finished: true,
        }
    }
}

#[derive(Clone)]
pub struct RunController {
    cancelled: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl RunController {
    pub fn cancel(&self) -> Result<bool, String> {
        self.cancelled.store(true, Ordering::Relaxed);

        let mut guard = self
            .child
            .lock()
            .map_err(|_| "run lock poisoned".to_string())?;
        let Some(child) = guard.as_mut() else {
            return Ok(false);
        };

        child
            .kill()
            .map(|_| true)
            .map_err(|error| error.to_string())
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub struct ManagedRun {
    pub receiver: mpsc::Receiver<RunnerUpdate>,
    pub controller: RunController,
}

pub fn next_session_id() -> String {
    let count = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("session-{}-{count}", unix_millis())
}

pub fn parse_event(line: &str) -> Option<Event> {
    let mut parts = line.splitn(3, '|');
    let timestamp = parts.next()?.trim().to_string();
    let kind = EventKind::parse(parts.next()?)?;
    let message = parts.next()?.trim().to_string();

    Some(Event::new(timestamp, kind, message))
}

pub fn parse_codex_event(line: &str) -> Event {
    parse_codex_events(line)
        .into_iter()
        .next()
        .unwrap_or_else(|| Event::new("stream", EventKind::Warning, "empty codex event"))
}

pub fn parse_codex_events(line: &str) -> Vec<Event> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return vec![Event::new(
            "stream",
            EventKind::Warning,
            format!("invalid json event: {line}"),
        )];
    };

    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let mut events = match event_type {
        "thread.started" => vec![parse_thread_started(&value)],
        "turn.started" => vec![Event::new("turn", EventKind::Status, "turn started")],
        "turn.completed" => vec![parse_turn_completed(&value)],
        "item.started" => parse_started_item(&value)
            .map(|event| vec![event])
            .unwrap_or_else(|| vec![fallback_event("event", event_type, &value)]),
        "item.completed" => parse_completed_item(&value)
            .map(|event| vec![event])
            .unwrap_or_else(|| vec![fallback_event("event", event_type, &value)]),
        _ => vec![fallback_event("event", event_type, &value)],
    };

    for path in extract_file_paths(&value) {
        let group = classify_file_group(&path);
        events.push(
            Event::new("file", EventKind::File, path.clone()).with_details(EventDetails {
                command: None,
                file: Some(FileDetails { path, group }),
            }),
        );
    }

    events
}

pub fn start_codex_run(request: RunRequest) -> ManagedRun {
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let child = Arc::new(Mutex::new(None));
    let controller = RunController {
        cancelled: Arc::clone(&cancelled),
        child: Arc::clone(&child),
    };

    thread::spawn(move || {
        let executable = if cfg!(windows) { "codex.cmd" } else { "codex" };
        let mut command = Command::new(executable);
        command.args(build_codex_args(&request));
        command.current_dir(request.workspace.as_str());

        let spawned = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();

        let mut process = match spawned {
            Ok(child_process) => child_process,
            Err(error) => {
                let event = Event::new(
                    "session",
                    EventKind::Error,
                    format!("failed to start codex: {error}"),
                );
                let _ = sender.send(RunnerUpdate::finished(event));
                return;
            }
        };

        let stdout = process.stdout.take();
        let stderr = process.stderr.take();
        {
            let mut guard = child.lock().ok();
            if let Some(guard) = guard.as_mut() {
                **guard = Some(process);
            }
        }

        let stdout_thread = stdout.map(|stdout| {
            let sender = sender.clone();
            thread::spawn(move || stream_codex_stdout(stdout, &sender))
        });

        let stderr_thread = stderr.map(|stderr| {
            let sender = sender.clone();
            thread::spawn(move || stream_codex_stderr(stderr, &sender))
        });

        let exit_status = wait_for_exit(&child, &cancelled);

        if let Some(handle) = stdout_thread {
            let _ = handle.join();
        }
        if let Some(handle) = stderr_thread {
            let _ = handle.join();
        }

        let event = final_event(&exit_status, cancelled.load(Ordering::Relaxed));
        let _ = sender.send(RunnerUpdate::finished(event));
    });

    ManagedRun {
        receiver,
        controller,
    }
}

pub fn spawn_codex_run(
    prompt: String,
    workspace: String,
    settings: RunSettings,
) -> mpsc::Receiver<RunnerUpdate> {
    start_codex_run(RunRequest {
        session_id: next_session_id(),
        prompt,
        workspace,
        settings,
        codex_session_id: None,
    })
    .receiver
}

fn build_codex_args(request: &RunRequest) -> Vec<String> {
    let mut args = Vec::new();

    if request.settings.bypass_approvals_and_sandbox {
        args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
    } else if !request.settings.approval.trim().is_empty() {
        args.push("--ask-for-approval".to_string());
        args.push(request.settings.approval.clone());
    }

    args.push("-C".to_string());
    args.push(request.workspace.clone());

    if !request.settings.model.trim().is_empty() {
        args.push("--model".to_string());
        args.push(request.settings.model.clone());
    }

    if !request.settings.bypass_approvals_and_sandbox && !request.settings.sandbox.trim().is_empty()
    {
        args.push("--sandbox".to_string());
        args.push(request.settings.sandbox.clone());
    }

    args.push("exec".to_string());
    if let Some(codex_session_id) = request.codex_session_id.as_deref() {
        args.push("resume".to_string());
        args.push(codex_session_id.to_string());
    }

    args.push("--json".to_string());
    args.push("--skip-git-repo-check".to_string());
    args.push(request.prompt.clone());
    args
}

pub fn compact_text(text: &str) -> String {
    compact_text_to(text, 88)
}

pub fn compact_text_to(text: &str, limit: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if compact.len() <= limit {
        compact
    } else {
        let safe_limit = limit.max(4);
        format!("{}...", &compact[..safe_limit - 3])
    }
}

fn parse_completed_item(value: &Value) -> Option<Event> {
    let item = value.get("item")?;
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "agent_message" => Some(Event::new(
            "agent",
            EventKind::Note,
            item.get("text")
                .and_then(Value::as_str)
                .unwrap_or("(empty agent message)"),
        )),
        "command_execution" => parse_command_execution(item, "completed"),
        _ => Some(Event::new(
            "item",
            EventKind::Note,
            format!("{item_type} completed"),
        )),
    }
}

fn parse_started_item(value: &Value) -> Option<Event> {
    let item = value.get("item")?;
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "command_execution" => parse_command_execution(item, "started"),
        _ => Some(Event::new(
            "item",
            EventKind::Note,
            format!("{item_type} started"),
        )),
    }
}

fn parse_thread_started(value: &Value) -> Event {
    Event::new(
        "thread",
        EventKind::Status,
        format!(
            "thread started: {}",
            value
                .get("thread_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
    )
}

fn parse_turn_completed(value: &Value) -> Event {
    let usage = value.get("usage");
    let input_tokens = usage
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Event::new(
        "turn",
        EventKind::Status,
        format!("turn completed: {input_tokens} input tokens, {output_tokens} output tokens"),
    )
}

fn parse_command_execution(item: &Value, stage: &str) -> Option<Event> {
    let command = item
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let exit_code = item.get("exit_code").and_then(Value::as_i64);
    let duration_ms = item
        .get("duration_ms")
        .or_else(|| item.get("elapsed_ms"))
        .and_then(Value::as_u64);
    let aggregated_output = item
        .get("aggregated_output")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let message = match (stage, exit_code, duration_ms, aggregated_output.as_deref()) {
        ("started", _, _, _) => format!("started: {}", compact_text(command)),
        ("completed", Some(code), Some(duration), Some(output)) => format!(
            "completed (exit {code}, {duration}ms): {} | {}",
            compact_text(command),
            compact_text(output)
        ),
        ("completed", Some(code), Some(duration), None) => {
            format!(
                "completed (exit {code}, {duration}ms): {}",
                compact_text(command)
            )
        }
        ("completed", Some(code), None, Some(output)) => format!(
            "completed (exit {code}): {} | {}",
            compact_text(command),
            compact_text(output)
        ),
        ("completed", Some(code), None, None) => {
            format!("completed (exit {code}): {}", compact_text(command))
        }
        ("completed", None, Some(duration), _) => {
            format!("completed ({duration}ms): {}", compact_text(command))
        }
        _ => format!("{stage}: {}", compact_text(command)),
    };

    Some(
        Event::new("command", EventKind::Command, message).with_details(EventDetails {
            command: Some(CommandDetails {
                command: command.to_string(),
                stage: stage.to_string(),
                exit_code,
                duration_ms,
                aggregated_output,
            }),
            file: None,
        }),
    )
}

fn fallback_event(timestamp: &str, event_type: &str, value: &Value) -> Event {
    Event::new(
        timestamp,
        EventKind::Note,
        format!("{event_type}: {}", compact_json(value)),
    )
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unserializable event>".to_string())
}

fn stream_codex_stdout(stdout: impl io::Read, sender: &mpsc::Sender<RunnerUpdate>) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = match reader.read_line(&mut line) {
            Ok(bytes_read) => bytes_read,
            Err(error) => {
                let _ = sender.send(RunnerUpdate::event(Event::new(
                    "stream",
                    EventKind::Warning,
                    format!("failed while reading codex output: {error}"),
                )));
                break;
            }
        };

        if bytes_read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        for event in parse_codex_events(trimmed) {
            let _ = sender.send(RunnerUpdate::event(event));
        }
    }
}

fn stream_codex_stderr(stderr: impl io::Read, sender: &mpsc::Sender<RunnerUpdate>) {
    let stderr_reader = BufReader::new(stderr);
    for result in stderr_reader.lines() {
        match result {
            Ok(stderr_line) if !stderr_line.trim().is_empty() => {
                let _ = sender.send(RunnerUpdate::event(Event::new(
                    "stderr",
                    EventKind::Warning,
                    stderr_line,
                )));
            }
            Ok(_) => {}
            Err(error) => {
                let _ = sender.send(RunnerUpdate::event(Event::new(
                    "stderr",
                    EventKind::Warning,
                    format!("stderr read error: {error}"),
                )));
            }
        }
    }
}

fn wait_for_exit(
    child: &Arc<Mutex<Option<Child>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<ExitStatus, String> {
    loop {
        {
            let mut guard = child.lock().map_err(|_| "run lock poisoned".to_string())?;
            let Some(process) = guard.as_mut() else {
                return Err("codex process handle missing".to_string());
            };

            match process.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {
                    if cancelled.load(Ordering::Relaxed) {
                        let _ = process.kill();
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }

        thread::sleep(Duration::from_millis(50));
    }
}

fn final_event(exit_status: &Result<ExitStatus, String>, cancelled: bool) -> Event {
    if cancelled {
        return Event::new("session", EventKind::Warning, "codex run cancelled");
    }

    match exit_status {
        Ok(status) if status.success() => Event::new(
            "session",
            EventKind::Status,
            "codex run completed successfully",
        ),
        Ok(status) => Event::new(
            "session",
            EventKind::Error,
            format!("codex run failed with status {status}"),
        ),
        Err(error) => Event::new(
            "session",
            EventKind::Error,
            format!("failed to wait for codex: {error}"),
        ),
    }
}

fn extract_file_paths(value: &Value) -> Vec<String> {
    let mut paths = BTreeSet::new();
    collect_paths(value, None, &mut paths);
    paths.into_iter().collect()
}

fn collect_paths(value: &Value, key: Option<&str>, paths: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (next_key, next_value) in map {
                collect_paths(next_value, Some(next_key.as_str()), paths);
            }
        }
        Value::Array(values) => {
            for item in values {
                collect_paths(item, key, paths);
            }
        }
        Value::String(text) => {
            let looks_like_path_key = matches!(
                key,
                Some("path" | "file_path" | "target_file" | "relative_path" | "workspace_path")
            );
            let looks_like_path_value = text.contains('/') || text.contains('\\');

            if looks_like_path_key && looks_like_path_value && !text.starts_with('{') {
                paths.insert(text.clone());
            }
        }
        _ => {}
    }
}

fn classify_file_group(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let path = Path::new(&normalized);

    if let Some(parent) = path.parent().and_then(Path::to_str) {
        if !parent.is_empty() && parent != "." {
            return parent.to_string();
        }
    }

    path.extension()
        .and_then(|value| value.to_str())
        .map(|ext| format!("*.{ext}"))
        .unwrap_or_else(|| "root".to_string())
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_command_duration_and_exit_status() {
        let line = r#"{"type":"item.completed","item":{"type":"command_execution","command":"cargo test","exit_code":1,"duration_ms":420,"aggregated_output":"failed"}} "#;
        let event = parse_codex_event(line.trim());
        let details = event.details.expect("command details");
        let command = details.command.expect("command payload");

        assert_eq!(event.kind, EventKind::Command);
        assert_eq!(command.command, "cargo test");
        assert_eq!(command.exit_code, Some(1));
        assert_eq!(command.duration_ms, Some(420));
    }

    #[test]
    fn summary_aggregates_command_and_file_analytics() {
        let mut summary = Summary::with_source("test");
        summary.record(
            Event::new("command", EventKind::Command, "completed").with_details(EventDetails {
                command: Some(CommandDetails {
                    command: "cargo test".to_string(),
                    stage: "completed".to_string(),
                    exit_code: Some(0),
                    duration_ms: Some(50),
                    aggregated_output: None,
                }),
                file: None,
            }),
        );
        summary.record(
            Event::new("file", EventKind::File, "desktop/src/app.ts").with_details(EventDetails {
                command: None,
                file: Some(FileDetails {
                    path: "desktop/src/app.ts".to_string(),
                    group: "desktop".to_string(),
                }),
            }),
        );

        assert_eq!(summary.analytics.command_runs.len(), 1);
        assert_eq!(summary.analytics.exit_status_counts.get("0"), Some(&1));
        assert_eq!(summary.analytics.file_groups.get("desktop"), Some(&1));
    }

    #[test]
    fn stream_stdout_emits_all_parsed_events() {
        let input = Cursor::new(
            r#"{"type":"thread.started","thread_id":"abc"}
{"type":"item.completed","item":{"type":"command_execution","command":"cargo test","exit_code":0,"duration_ms":12,"aggregated_output":"ok","path":"desktop/src/app.ts"}}"#,
        );
        let (sender, receiver) = mpsc::channel();

        stream_codex_stdout(input, &sender);
        drop(sender);

        let events = receiver.into_iter().collect::<Vec<_>>();
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .any(|update| update.event.kind == EventKind::Command));
        assert!(events
            .iter()
            .any(|update| update.event.kind == EventKind::File));
    }

    #[test]
    fn fresh_exec_args_do_not_include_resume() {
        let args = build_codex_args(&RunRequest {
            session_id: "run-1".to_string(),
            prompt: "fix tests".to_string(),
            workspace: "c:/repo".to_string(),
            settings: RunSettings::default(),
            codex_session_id: None,
        });

        assert!(args.windows(2).all(|window| window != ["exec", "resume"]));
        assert!(args.contains(&"exec".to_string()));
        assert!(!args.contains(&"resume".to_string()));
    }

    #[test]
    fn bypass_exec_args_skip_approval_and_sandbox_flags() {
        let args = build_codex_args(&RunRequest {
            session_id: "run-1".to_string(),
            prompt: "fix tests".to_string(),
            workspace: "c:/repo".to_string(),
            settings: RunSettings {
                model: "gpt-5.2-codex".to_string(),
                sandbox: "danger-full-access".to_string(),
                approval: "never".to_string(),
                bypass_approvals_and_sandbox: true,
            },
            codex_session_id: None,
        });

        assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert!(!args.contains(&"--ask-for-approval".to_string()));
        assert!(!args.contains(&"--sandbox".to_string()));
    }

    #[test]
    fn resumed_exec_args_include_resume_and_session_id() {
        let args = build_codex_args(&RunRequest {
            session_id: "run-1".to_string(),
            prompt: "continue with more tests".to_string(),
            workspace: "c:/repo".to_string(),
            settings: RunSettings::default(),
            codex_session_id: Some("019ccdee-5bdb-7602-95df-d6edbfd0083c".to_string()),
        });

        let resume_index = args.iter().position(|value| value == "resume").expect("resume argument");
        assert_eq!(args[resume_index + 1], "019ccdee-5bdb-7602-95df-d6edbfd0083c");
        assert_eq!(args[resume_index - 1], "exec");
    }
}
