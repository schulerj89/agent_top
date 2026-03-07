use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Stdout};
use std::process::{self, Command, Stdio};

use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Terminal;
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

impl Event {
    fn new(timestamp: impl Into<String>, kind: EventKind, message: impl Into<String>) -> Self {
        Self {
            timestamp: timestamp.into(),
            kind,
            message: message.into(),
        }
    }
}

#[derive(Default)]
struct Summary {
    source: String,
    current_status: Option<String>,
    commands: usize,
    warnings: usize,
    errors: usize,
    files_touched: BTreeSet<String>,
    recent_events: Vec<Event>,
    total_events: usize,
}

impl Summary {
    fn with_source(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            ..Self::default()
        }
    }

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

    let mut terminal = init_terminal();
    let mut summary = Summary::with_source(path);

    for (line_number, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let Some(event) = parse_event(line) else {
            eprintln!("skipping invalid line {}: {}", line_number + 1, line);
            continue;
        };

        record_and_render(&mut terminal, &mut summary, event);
    }
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

    let mut terminal = init_terminal();
    let mut summary = Summary::with_source("live codex session");
    let mut stdout_reader = BufReader::new(stdout);
    let mut line = String::new();

    render_dashboard(&mut terminal, &summary);

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

        let event = parse_codex_event(trimmed);
        record_and_render(&mut terminal, &mut summary, event);
    }

    collect_stderr_events(stderr, &mut terminal, &mut summary);

    let exit_status = child.wait().unwrap_or_else(|error| {
        eprintln!("failed to wait for codex: {error}");
        process::exit(1);
    });

    let final_event = if exit_status.success() {
        Event::new("session", EventKind::Status, "codex run completed successfully")
    } else {
        Event::new(
            "session",
            EventKind::Error,
            format!("codex run failed with status {exit_status}"),
        )
    };

    record_and_render(&mut terminal, &mut summary, final_event);
}

fn parse_event(line: &str) -> Option<Event> {
    let mut parts = line.splitn(3, '|');
    let timestamp = parts.next()?.trim().to_string();
    let kind = EventKind::parse(parts.next()?)?;
    let message = parts.next()?.trim().to_string();

    Some(Event::new(timestamp, kind, message))
}

fn parse_codex_event(line: &str) -> Event {
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
        ("started", _, _) => format!("started: {}", compact_command(command)),
        ("completed", Some(code), true) => {
            format!("completed (exit {}): {}", code, compact_command(command))
        }
        ("completed", Some(code), false) => format!(
            "completed (exit {}): {} | {}",
            code,
            compact_command(command),
            compact_text(aggregated_output)
        ),
        ("completed", None, _) => format!("completed: {}", compact_command(command)),
        _ => format!("{}: {}", stage, compact_command(command)),
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

fn compact_command(command: &str) -> String {
    compact_text(command)
}

fn compact_text(text: &str) -> String {
    const LIMIT: usize = 88;
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if compact.len() <= LIMIT {
        compact
    } else {
        format!("{}...", &compact[..LIMIT - 3])
    }
}

fn collect_stderr_events(
    stderr: impl io::Read,
    terminal: &mut AppTerminal,
    summary: &mut Summary,
) {
    let stderr_reader = BufReader::new(stderr);
    for result in stderr_reader.lines() {
        match result {
            Ok(stderr_line) if !stderr_line.trim().is_empty() => {
                record_and_render(
                    terminal,
                    summary,
                    Event::new("stderr", EventKind::Warning, stderr_line),
                );
            }
            Ok(_) => {}
            Err(error) => {
                record_and_render(
                    terminal,
                    summary,
                    Event::new(
                        "stderr",
                        EventKind::Warning,
                        format!("stderr read error: {error}"),
                    ),
                );
            }
        }
    }
}

fn record_and_render(terminal: &mut AppTerminal, summary: &mut Summary, event: Event) {
    summary.record(event);
    render_dashboard(terminal, summary);
}

fn init_terminal() -> AppTerminal {
    let mut stdout = io::stdout();
    let _ = enable_raw_mode();
    let _ = execute!(stdout, Hide);

    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).unwrap_or_else(|error| {
        eprintln!("failed to initialize terminal UI: {error}");
        process::exit(1);
    });

    AppTerminal { terminal }
}

fn render_dashboard(terminal: &mut AppTerminal, summary: &Summary) {
    let _ = terminal.terminal.draw(|frame| {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Min(8),
                Constraint::Length(10),
            ])
            .split(frame.area());

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(vertical[0]);

        let middle = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(vertical[1]);

        frame.render_widget(render_overview(summary), top[0]);
        frame.render_widget(render_metrics(summary), top[1]);
        frame.render_widget(render_files(summary), middle[0]);
        frame.render_widget(render_events(summary), middle[1]);
        frame.render_widget(render_footer(summary), vertical[2]);
    });
}

fn render_overview(summary: &Summary) -> Paragraph<'static> {
    let lines = vec![
        Line::from(vec![
            Span::styled("agent_top", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  live session tracker"),
        ]),
        Line::from(format!("Source: {}", summary.source)),
        Line::from(format!(
            "Status: {}",
            summary.current_status.as_deref().unwrap_or("unknown")
        )),
    ];

    Paragraph::new(lines)
        .block(Block::default().title("Overview").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}

fn render_metrics(summary: &Summary) -> Paragraph<'static> {
    let lines = vec![
        metric_line("Events", summary.total_events, Color::Blue),
        metric_line("Commands", summary.commands, Color::Yellow),
        metric_line("Warnings", summary.warnings, Color::LightYellow),
        metric_line("Errors", summary.errors, Color::LightRed),
        metric_line("Files", summary.files_touched.len(), Color::Green),
    ];

    Paragraph::new(lines)
        .block(Block::default().title("Metrics").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}

fn metric_line(label: &str, value: usize, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<8}"),
            Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), Style::default().fg(color)),
    ])
}

fn render_files(summary: &Summary) -> List<'static> {
    let items = if summary.files_touched.is_empty() {
        vec![ListItem::new(Line::from("(none)"))]
    } else {
        summary
            .files_touched
            .iter()
            .map(|file| ListItem::new(Line::from(file.clone())))
            .collect()
    };

    List::new(items).block(Block::default().title("Tracked Files").borders(Borders::ALL))
}

fn render_events(summary: &Summary) -> List<'static> {
    let items = if summary.recent_events.is_empty() {
        vec![ListItem::new(Line::from("(none)"))]
    } else {
        summary
            .recent_events
            .iter()
            .map(render_event_item)
            .collect()
    };

    List::new(items).block(Block::default().title("Recent Events").borders(Borders::ALL))
}

fn render_event_item(event: &Event) -> ListItem<'static> {
    let style = event_style(event.kind);
    let line = Line::from(vec![
        Span::styled(
            format!("{:<10}", event.timestamp),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            format!("{:<8}", event.kind.label()),
            style.add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(event.message.clone(), style),
    ]);

    ListItem::new(line)
}

fn render_footer(summary: &Summary) -> Paragraph<'static> {
    let last = summary
        .recent_events
        .last()
        .map(|event| format!("Last event: {} {}", event.kind.label(), event.message))
        .unwrap_or_else(|| "Last event: none".to_string());

    Paragraph::new(vec![
        Line::from(last),
        Line::from("Press Ctrl+C to exit a live session."),
    ])
    .block(Block::default().title("Log").borders(Borders::ALL))
    .wrap(Wrap { trim: true })
}

fn event_style(kind: EventKind) -> Style {
    match kind {
        EventKind::Status => Style::default().fg(Color::Cyan),
        EventKind::Command => Style::default().fg(Color::Yellow),
        EventKind::File => Style::default().fg(Color::Green),
        EventKind::Warning => Style::default().fg(Color::LightYellow),
        EventKind::Error => Style::default().fg(Color::LightRed),
        EventKind::Note => Style::default().fg(Color::White),
    }
}

struct AppTerminal {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Drop for AppTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), Show);
        let _ = self.terminal.show_cursor();
    }
}
