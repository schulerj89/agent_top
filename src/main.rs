use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process;

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
}

impl Summary {
    fn record(&mut self, event: Event) {
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
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: agent_top <event-log-path>");
        process::exit(1);
    });

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

fn render_dashboard(summary: &Summary, path: &str) {
    println!("agent_top");
    println!("{}", "=".repeat(72));
    println!("source        : {path}");
    println!(
        "status        : {}",
        summary.current_status.as_deref().unwrap_or("unknown")
    );
    println!("commands      : {}", summary.commands);
    println!("warnings      : {}", summary.warnings);
    println!("errors        : {}", summary.errors);
    println!("files touched : {}", summary.files_touched.len());
    println!();

    println!("tracked files");
    println!("{}", "-".repeat(72));
    if summary.files_touched.is_empty() {
        println!("(none)");
    } else {
        for file in &summary.files_touched {
            println!("{file}");
        }
    }

    println!();
    println!("recent events");
    println!("{}", "-".repeat(72));
    if summary.recent_events.is_empty() {
        println!("(none)");
    } else {
        for event in &summary.recent_events {
            println!(
                "{:<19} {:<8} {}",
                event.timestamp,
                event.kind.label(),
                event.message
            );
        }
    }
}
