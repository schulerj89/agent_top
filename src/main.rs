use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{self, Command, Stdio};

use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventKind {
    Status,
    Command,
    File,
    Warning,
    Error,
    Note,
}

impl EventKind {
    fn parse(value: &str) -> Option<Self> {
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

    fn label(self) -> &'static str {
        match self {
            Self::Status => "STATUS",
            Self::Command => "COMMAND",
            Self::File => "FILE",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Note => "NOTE",
        }
    }
}

#[derive(Debug)]
struct Event {
    timestamp: String,
    kind: EventKind,
    message: String,
}

#[derive(Default)]
struct Summary {
    current_status: Option<String>,
    commands: usize,
    warnings: usize,
    errors: usize,
    files_touched: BTreeSet<String>,
    recent_events: Vec<Event>,
    total_events: usize,
}

impl Summary {
    fn record(&mut self, event: Event) {
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

fn main() {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| {
        eprintln!("usage: agent_top <replay <event-log-path> | run <prompt>>");
        process::exit(1);
    });

    match command.as_str() {
        "replay" => {
            let path = args.next().unwrap_or_else(|| {
                eprintln!("usage: agent_top replay <event-log-path>");
                process::exit(1);
            });
            replay_log(&path);
        }
        "run" => {
            let prompt = args.collect::<Vec<_>>().join(" ");
            if prompt.trim().is_empty() {
                eprintln!("usage: agent_top run <prompt>");
                process::exit(1);
            }
            run_codex(&prompt);
        }
        path => {
            replay_log(path);
        }
    }
}

fn replay_log(path: &str) {
    let contents = fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("failed to read {path}: {error}");
        process::exit(1);
    });

    let mut summary = Summary::default();

    for (line_number, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let Some(event) = parse_event(line) else {
            eprintln!("skipping invalid line {}: {}", line_number + 1, line);
            continue;
        };

        summary.record(event);
    }

    render_dashboard(&summary, &path);
}

fn run_codex(prompt: &str) {
    let workspace = env::current_dir().unwrap_or_else(|error| {
        eprintln!("failed to resolve current directory: {error}");
        process::exit(1);
    });

    let executable = if cfg!(windows) { "codex.cmd" } else { "codex" };

    let mut child = Command::new(executable)
        .args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-C",
            workspace.to_string_lossy().as_ref(),
            prompt,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| {
            eprintln!("failed to start codex: {error}");
            process::exit(1);
        });

    let stdout = child.stdout.take().unwrap_or_else(|| {
        eprintln!("failed to capture codex stdout");
        process::exit(1);
    });

    let stderr = child.stderr.take().unwrap_or_else(|| {
        eprintln!("failed to capture codex stderr");
        process::exit(1);
    });

    let mut summary = Summary::default();
    let mut stderr_lines = Vec::new();
    let mut stdout_reader = BufReader::new(stdout);
    let mut line = String::new();

    render_dashboard(&summary, "live codex session");

    loop {
        line.clear();
        let bytes_read = stdout_reader.read_line(&mut line).unwrap_or_else(|error| {
            eprintln!("failed while reading codex output: {error}");
            process::exit(1);
        });

        if bytes_read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        summary.record(parse_codex_event(trimmed));
        render_dashboard(&summary, "live codex session");
    }

    let stderr_reader = BufReader::new(stderr);
    for result in stderr_reader.lines() {
        match result {
            Ok(stderr_line) if !stderr_line.trim().is_empty() => stderr_lines.push(stderr_line),
            Ok(_) => {}
            Err(error) => stderr_lines.push(format!("stderr read error: {error}")),
        }
    }

    let exit_status = child.wait().unwrap_or_else(|error| {
        eprintln!("failed to wait for codex: {error}");
        process::exit(1);
    });

    let final_status = if exit_status.success() {
        "codex run completed successfully".to_string()
    } else {
        format!("codex run failed with status {exit_status}")
    };

    summary.record(Event {
        timestamp: "session".to_string(),
        kind: if exit_status.success() {
            EventKind::Status
        } else {
            EventKind::Error
        },
        message: final_status,
    });

    for stderr_line in stderr_lines {
        summary.record(Event {
            timestamp: "stderr".to_string(),
            kind: EventKind::Warning,
            message: stderr_line,
        });
    }

    render_dashboard(&summary, "live codex session");
}

fn parse_event(line: &str) -> Option<Event> {
    let mut parts = line.splitn(3, '|');
    let timestamp = parts.next()?.trim().to_string();
    let kind = EventKind::parse(parts.next()?)?;
    let message = parts.next()?.trim().to_string();

    Some(Event {
        timestamp,
        kind,
        message,
    })
}

fn parse_codex_event(line: &str) -> Event {
    let fallback = Event {
        timestamp: "stream".to_string(),
        kind: EventKind::Note,
        message: line.to_string(),
    };

    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Event {
            timestamp: "stream".to_string(),
            kind: EventKind::Warning,
            message: format!("invalid json event: {line}"),
        };
    };

    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    match event_type {
        "thread.started" => Event {
            timestamp: "thread".to_string(),
            kind: EventKind::Status,
            message: format!(
                "thread started: {}",
                value
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ),
        },
        "turn.started" => Event {
            timestamp: "turn".to_string(),
            kind: EventKind::Status,
            message: "turn started".to_string(),
        },
        "turn.completed" => {
            let usage = value.get("usage");
            let input_tokens = usage
                .and_then(|usage| usage.get("input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let output_tokens = usage
                .and_then(|usage| usage.get("output_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);

            Event {
                timestamp: "turn".to_string(),
                kind: EventKind::Status,
                message: format!(
                    "turn completed: {} input tokens, {} output tokens",
                    input_tokens, output_tokens
                ),
            }
        }
        "item.completed" => parse_completed_item(&value).unwrap_or(fallback),
        _ => Event {
            timestamp: "event".to_string(),
            kind: EventKind::Note,
            message: format!("{}: {}", event_type, compact_json(&value)),
        },
    }
}

fn parse_completed_item(value: &Value) -> Option<Event> {
    let item = value.get("item")?;
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "agent_message" => Some(Event {
            timestamp: "agent".to_string(),
            kind: EventKind::Note,
            message: item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("(empty agent message)")
                .to_string(),
        }),
        _ => Some(Event {
            timestamp: "item".to_string(),
            kind: EventKind::Note,
            message: format!("{} completed", item_type),
        }),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unserializable event>".to_string())
}

fn render_dashboard(summary: &Summary, path: &str) {
    let mut stdout = io::stdout();
    let _ = write!(stdout, "\x1b[2J\x1b[H");
    let _ = writeln!(stdout, "agent_top");
    let _ = writeln!(stdout, "{}", "=".repeat(72));
    let _ = writeln!(stdout, "source        : {path}");
    let _ = writeln!(
        stdout,
        "status        : {}",
        summary.current_status.as_deref().unwrap_or("unknown")
    );
    let _ = writeln!(stdout, "events        : {}", summary.total_events);
    let _ = writeln!(stdout, "commands      : {}", summary.commands);
    let _ = writeln!(stdout, "warnings      : {}", summary.warnings);
    let _ = writeln!(stdout, "errors        : {}", summary.errors);
    let _ = writeln!(stdout, "files touched : {}", summary.files_touched.len());
    let _ = writeln!(stdout);

    let _ = writeln!(stdout, "tracked files");
    let _ = writeln!(stdout, "{}", "-".repeat(72));
    if summary.files_touched.is_empty() {
        let _ = writeln!(stdout, "(none)");
    } else {
        for file in &summary.files_touched {
            let _ = writeln!(stdout, "{file}");
        }
    }

    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "recent events");
    let _ = writeln!(stdout, "{}", "-".repeat(72));
    if summary.recent_events.is_empty() {
        let _ = writeln!(stdout, "(none)");
    } else {
        for event in &summary.recent_events {
            let _ = writeln!(
                stdout,
                "{:<19} {:<8} {}",
                event.timestamp,
                event.kind.label(),
                event.message
            );
        }
    }
    let _ = stdout.flush();
}
