use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug)]
pub struct Event {
    pub timestamp: String,
    pub kind: EventKind,
    pub message: String,
}

impl Event {
    pub fn new(timestamp: impl Into<String>, kind: EventKind, message: impl Into<String>) -> Self {
        Self {
            timestamp: timestamp.into(),
            kind,
            message: message.into(),
        }
    }
}

#[derive(Default)]
pub struct Summary {
    pub source: String,
    pub current_status: Option<String>,
    pub commands: usize,
    pub warnings: usize,
    pub errors: usize,
    pub files_touched: BTreeSet<String>,
    pub recent_events: Vec<Event>,
    pub total_events: usize,
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

        self.recent_events.push(event);
        if self.recent_events.len() > 8 {
            self.recent_events.remove(0);
        }
    }
}

#[derive(Clone)]
pub struct RunSettings {
    pub model: String,
    pub sandbox: String,
    pub approval: String,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            model: String::new(),
            sandbox: "workspace-write".to_string(),
            approval: "never".to_string(),
        }
    }
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

pub fn parse_event(line: &str) -> Option<Event> {
    let mut parts = line.splitn(3, '|');
    let timestamp = parts.next()?.trim().to_string();
    let kind = EventKind::parse(parts.next()?)?;
    let message = parts.next()?.trim().to_string();

    Some(Event::new(timestamp, kind, message))
}

pub fn parse_codex_event(line: &str) -> Event {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Event::new(
            "stream",
            EventKind::Warning,
            format!("invalid json event: {line}"),
        );
    };

    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    match event_type {
        "thread.started" => parse_thread_started(&value),
        "turn.started" => Event::new("turn", EventKind::Status, "turn started"),
        "turn.completed" => parse_turn_completed(&value),
        "item.started" => parse_started_item(&value)
            .unwrap_or_else(|| fallback_event("event", event_type, &value)),
        "item.completed" => parse_completed_item(&value)
            .unwrap_or_else(|| fallback_event("event", event_type, &value)),
        _ => fallback_event("event", event_type, &value),
    }
}

pub fn spawn_codex_run(prompt: String, workspace: String, settings: RunSettings) -> Receiver<RunnerUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let executable = if cfg!(windows) { "codex.cmd" } else { "codex" };
        let mut command = Command::new(executable);

        if !settings.approval.trim().is_empty() {
            command.arg("--ask-for-approval").arg(settings.approval.as_str());
        }

        command
            .arg("exec")
            .arg("--json")
            .arg("--skip-git-repo-check");

        if !settings.model.trim().is_empty() {
            command.arg("--model").arg(settings.model.as_str());
        }

        if !settings.sandbox.trim().is_empty() {
            command.arg("--sandbox").arg(settings.sandbox.as_str());
        }

        let mut child = match command
            .arg("-C")
            .arg(workspace.as_str())
            .arg(prompt.as_str())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = sender.send(RunnerUpdate::finished(Event::new(
                    "session",
                    EventKind::Error,
                    format!("failed to start codex: {error}"),
                )));
                return;
            }
        };

        let Some(stdout) = child.stdout.take() else {
            let _ = sender.send(RunnerUpdate::finished(Event::new(
                "session",
                EventKind::Error,
                "failed to capture codex stdout",
            )));
            return;
        };

        let Some(stderr) = child.stderr.take() else {
            let _ = sender.send(RunnerUpdate::finished(Event::new(
                "session",
                EventKind::Error,
                "failed to capture codex stderr",
            )));
            return;
        };

        stream_codex_stdout(stdout, &sender);
        stream_codex_stderr(stderr, &sender);

        let exit_status = match child.wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = sender.send(RunnerUpdate::finished(Event::new(
                    "session",
                    EventKind::Error,
                    format!("failed to wait for codex: {error}"),
                )));
                return;
            }
        };

        let event = if exit_status.success() {
            Event::new("session", EventKind::Status, "codex run completed successfully")
        } else {
            Event::new(
                "session",
                EventKind::Error,
                format!("codex run failed with status {exit_status}"),
            )
        };

        let _ = sender.send(RunnerUpdate::finished(event));
    });

    receiver
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
            format!("{} completed", item_type),
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
            format!("{} started", item_type),
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
        format!(
            "turn completed: {} input tokens, {} output tokens",
            input_tokens, output_tokens
        ),
    )
}

fn parse_command_execution(item: &Value, stage: &str) -> Option<Event> {
    let command = item.get("command").and_then(Value::as_str).unwrap_or("unknown");
    let exit_code = item.get("exit_code").and_then(Value::as_i64);
    let aggregated_output = item
        .get("aggregated_output")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();

    let message = match (stage, exit_code, aggregated_output.is_empty()) {
        ("started", _, _) => format!("started: {}", compact_text(command)),
        ("completed", Some(code), true) => {
            format!("completed (exit {}): {}", code, compact_text(command))
        }
        ("completed", Some(code), false) => format!(
            "completed (exit {}): {} | {}",
            code,
            compact_text(command),
            compact_text(aggregated_output)
        ),
        ("completed", None, _) => format!("completed: {}", compact_text(command)),
        _ => format!("{}: {}", stage, compact_text(command)),
    };

    Some(Event::new("command", EventKind::Command, message))
}

fn fallback_event(timestamp: &str, event_type: &str, value: &Value) -> Event {
    Event::new(
        timestamp,
        EventKind::Note,
        format!("{}: {}", event_type, compact_json(value)),
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

        let _ = sender.send(RunnerUpdate::event(parse_codex_event(trimmed)));
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
