use std::env;
use std::fs;
use std::io::{self, Stdout};
use std::process;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use agent_top_core::{
    compact_text_to, next_session_id, parse_event, start_codex_run, Event, EventKind, ManagedRun,
    RunController, RunRequest, RunSettings, RunnerUpdate, Summary,
};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Terminal;
fn main() {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        run_app();
        return;
    };

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

    let mut terminal = init_terminal();
    let mut summary = Summary::with_source("live codex session");
    let managed = start_codex_run(RunRequest {
        session_id: next_session_id(),
        prompt: prompt.to_string(),
        workspace: workspace.to_string_lossy().into_owned(),
        settings: RunSettings::default(),
        codex_session_id: None,
    });

    render_dashboard(&mut terminal, &summary);
    drain_runner_events(&managed.receiver, &mut terminal, &mut summary);
}

fn run_app() {
    let workspace = env::current_dir().unwrap_or_else(|error| {
        eprintln!("failed to resolve current directory: {error}");
        process::exit(1);
    });

    let mut terminal = init_terminal();
    let mut app = App::new(workspace.to_string_lossy().into_owned());

    loop {
        render_app(&mut terminal, &app);

        let mut run_finished = false;
        if let Some(receiver) = &app.receiver {
            while let Ok(update) = receiver.try_recv() {
                app.summary.record(update.event);
                if update.finished {
                    app.mode = AppMode::Ready;
                    run_finished = true;
                }
            }
        }
        if run_finished {
            app.receiver = None;
            app.controller = None;
        }

        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            let Ok(CEvent::Key(key)) = event::read() else {
                continue;
            };

            if key.kind != KeyEventKind::Press {
                continue;
            }

            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                break;
            }

            match app.mode {
                AppMode::Ready => handle_ready_key(&mut app, key.code),
                AppMode::EditingPrompt => handle_prompt_key(&mut app, key.code),
                AppMode::EditingSettings => handle_settings_key(&mut app, key.code),
                AppMode::Running => handle_running_key(&mut app, key.code),
            }
        }

        if app.should_quit {
            break;
        }
    }
}

fn record_and_render(terminal: &mut AppTerminal, summary: &mut Summary, event: Event) {
    summary.record(event);
    render_dashboard(terminal, summary);
}

fn drain_runner_events(
    receiver: &Receiver<RunnerUpdate>,
    terminal: &mut AppTerminal,
    summary: &mut Summary,
) {
    while let Ok(update) = receiver.recv() {
        record_and_render(terminal, summary, update.event);
        if update.finished {
            break;
        }
    }
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
        let layout = layout_mode(frame.area());
        let (top, middle) = split_top_middle(frame.area(), layout, false);

        frame.render_widget(render_overview(summary, top[0].width), top[0]);
        frame.render_widget(render_metrics(summary), top[1]);
        frame.render_widget(render_files(summary), middle[0]);
        frame.render_widget(render_events(summary, middle[1].width), middle[1]);
        frame.render_widget(
            render_footer(summary, frame.area().width),
            footer_area(frame.area(), false),
        );
    });
}

fn render_overview(summary: &Summary, width: u16) -> Paragraph<'static> {
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "agent_top",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  live session tracker"),
        ]),
        Line::from(format!(
            "Source: {}",
            compact_text_to(&summary.source, content_limit(width, 18))
        )),
        Line::from(format!(
            "Status: {}",
            compact_text_to(
                summary.current_status.as_deref().unwrap_or("unknown"),
                content_limit(width, 18),
            )
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
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
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

    List::new(items).block(
        Block::default()
            .title("Tracked Files")
            .borders(Borders::ALL),
    )
}

fn render_events(summary: &Summary, width: u16) -> List<'static> {
    let items = if summary.recent_events.is_empty() {
        vec![ListItem::new(Line::from("(none)"))]
    } else {
        summary
            .recent_events
            .iter()
            .map(|event| render_event_item(event, width))
            .collect()
    };

    List::new(items).block(
        Block::default()
            .title("Recent Events")
            .borders(Borders::ALL),
    )
}

fn render_event_item(event: &Event, width: u16) -> ListItem<'static> {
    let style = event_style(event.kind);
    let timestamp = compact_timestamp(&event.timestamp);
    let message_limit = content_limit(width, 22);
    let line = Line::from(vec![
        Span::styled(
            format!("{:<8}", timestamp),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            format!("{:<4}", short_kind_label(event.kind)),
            style.add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(compact_text_to(&event.message, message_limit), style),
    ]);

    ListItem::new(line)
}

fn render_footer(summary: &Summary, width: u16) -> Paragraph<'static> {
    let last = summary
        .recent_events
        .last()
        .map(|event| {
            format!(
                "Last: {} {}",
                short_kind_label(event.kind),
                compact_text_to(&event.message, content_limit(width, 14))
            )
        })
        .unwrap_or_else(|| "Last event: none".to_string());

    Paragraph::new(vec![Line::from(last), Line::from("Ctrl+C exits.")])
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

struct App {
    mode: AppMode,
    prompt_input: String,
    settings: RunSettings,
    settings_field: SettingsField,
    summary: Summary,
    receiver: Option<Receiver<RunnerUpdate>>,
    controller: Option<RunController>,
    last_request: Option<RunRequest>,
    workspace: String,
    should_quit: bool,
}

impl App {
    fn new(workspace: String) -> Self {
        Self {
            mode: AppMode::Ready,
            prompt_input: String::new(),
            settings: RunSettings::default(),
            settings_field: SettingsField::Model,
            summary: Summary::with_source("idle"),
            receiver: None,
            controller: None,
            last_request: None,
            workspace,
            should_quit: false,
        }
    }

    fn start_run(&mut self) {
        let prompt = self.prompt_input.trim().to_string();
        if prompt.is_empty() {
            return;
        }

        self.summary = Summary::with_source("live codex session");
        self.summary
            .record(Event::new("app", EventKind::Status, "launching codex run"));
        let request = RunRequest {
            session_id: next_session_id(),
            prompt,
            workspace: self.workspace.clone(),
            settings: self.settings.clone(),
            codex_session_id: None,
        };
        let ManagedRun {
            receiver,
            controller,
        } = start_codex_run(request.clone());
        self.last_request = Some(request);
        self.receiver = Some(receiver);
        self.controller = Some(controller);
        self.mode = AppMode::Running;
        self.prompt_input.clear();
    }

    fn retry_last_run(&mut self) {
        let Some(previous) = self.last_request.clone() else {
            return;
        };

        let request = RunRequest {
            session_id: next_session_id(),
            ..previous
        };
        let ManagedRun {
            receiver,
            controller,
        } = start_codex_run(request.clone());
        self.summary = Summary::with_source("live codex session");
        self.summary
            .record(Event::new("app", EventKind::Status, "retrying codex run"));
        self.last_request = Some(request);
        self.receiver = Some(receiver);
        self.controller = Some(controller);
        self.mode = AppMode::Running;
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AppMode {
    Ready,
    EditingPrompt,
    EditingSettings,
    Running,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SettingsField {
    Model,
    Sandbox,
    Approval,
}

impl SettingsField {
    fn next(self) -> Self {
        match self {
            Self::Model => Self::Sandbox,
            Self::Sandbox => Self::Approval,
            Self::Approval => Self::Model,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Model => Self::Approval,
            Self::Sandbox => Self::Model,
            Self::Approval => Self::Sandbox,
        }
    }
}

fn handle_ready_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('n') => app.mode = AppMode::EditingPrompt,
        KeyCode::Char('r') => app.retry_last_run(),
        KeyCode::Char('s') => app.mode = AppMode::EditingSettings,
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }
}

fn handle_prompt_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.mode = AppMode::Ready,
        KeyCode::Enter => app.start_run(),
        KeyCode::Backspace => {
            app.prompt_input.pop();
        }
        KeyCode::Char(ch) => app.prompt_input.push(ch),
        KeyCode::Tab => app.prompt_input.push(' '),
        _ => {}
    }
}

fn handle_settings_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.mode = AppMode::Ready,
        KeyCode::Up => app.settings_field = app.settings_field.previous(),
        KeyCode::Down => app.settings_field = app.settings_field.next(),
        KeyCode::Tab => app.settings_field = app.settings_field.next(),
        KeyCode::BackTab => app.settings_field = app.settings_field.previous(),
        KeyCode::Backspace => {
            selected_setting_mut(&mut app.settings, app.settings_field).pop();
        }
        KeyCode::Char(ch) => selected_setting_mut(&mut app.settings, app.settings_field).push(ch),
        _ => {}
    }
}

fn handle_running_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('c') => {
            if let Some(controller) = &app.controller {
                let message = match controller.cancel() {
                    Ok(true) => "cancellation requested for active codex run",
                    Ok(false) => "run already finished before cancellation",
                    Err(error) => {
                        return app
                            .summary
                            .record(Event::new("app", EventKind::Error, error))
                    }
                };
                app.summary
                    .record(Event::new("app", EventKind::Warning, message));
            }
        }
        KeyCode::Char('q') => {
            app.summary.record(Event::new(
                "app",
                EventKind::Warning,
                "quit ignored while codex run is active",
            ));
        }
        _ => {}
    }
}

fn render_app(terminal: &mut AppTerminal, app: &App) {
    let _ = terminal.terminal.draw(|frame| {
        let layout = layout_mode(frame.area());
        let (top, middle) = split_top_middle(frame.area(), layout, true);
        let controls = controls_area(frame.area(), layout);

        frame.render_widget(render_overview(&app.summary, top[0].width), top[0]);
        frame.render_widget(render_metrics(&app.summary), top[1]);
        frame.render_widget(render_launcher(app), controls[0]);
        frame.render_widget(render_settings(app), controls[1]);
        frame.render_widget(render_files(&app.summary), middle[0]);
        frame.render_widget(render_events(&app.summary, middle[1].width), middle[1]);
        frame.render_widget(render_help(app), footer_area(frame.area(), true));
    });
}

fn render_launcher(app: &App) -> Paragraph<'static> {
    let title = match app.mode {
        AppMode::Ready => "Launcher",
        AppMode::EditingPrompt => "New Run",
        AppMode::EditingSettings => "Launcher",
        AppMode::Running => "Run In Progress",
    };

    let body = match app.mode {
        AppMode::Ready => vec![
            Line::from("Press n to start a new Codex run."),
            Line::from(format!(
                "Workspace: {}",
                compact_text_to(&app.workspace, 32)
            )),
            Line::from("Press s to edit settings."),
            Line::from("Press r to retry the last completed run."),
        ],
        AppMode::EditingPrompt => vec![
            Line::from("Type a prompt and press Enter to launch Codex."),
            Line::from(vec![
                Span::styled("Prompt: ", Style::default().fg(Color::Cyan)),
                Span::raw(app.prompt_input.clone()),
            ]),
            Line::from("Esc cancels."),
        ],
        AppMode::EditingSettings => vec![
            Line::from("Edit settings in the panel to the right."),
            Line::from("Up/Down switch fields."),
            Line::from("Esc returns to the home screen."),
        ],
        AppMode::Running => vec![
            Line::from("Codex is running in the background."),
            Line::from("Recent events continue updating below."),
            Line::from("Press c to cancel the active run."),
        ],
    };

    Paragraph::new(body)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}

fn render_settings(app: &App) -> Paragraph<'static> {
    let fields = vec![
        settings_line(
            "Model",
            &app.settings.model,
            app.settings_field,
            SettingsField::Model,
            app.mode,
        ),
        settings_line(
            "Sandbox",
            &app.settings.sandbox,
            app.settings_field,
            SettingsField::Sandbox,
            app.mode,
        ),
        settings_line(
            "Approval",
            &app.settings.approval,
            app.settings_field,
            SettingsField::Approval,
            app.mode,
        ),
    ];

    Paragraph::new(fields)
        .block(Block::default().title("Settings").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}

fn settings_line(
    label: &str,
    value: &str,
    selected: SettingsField,
    field: SettingsField,
    mode: AppMode,
) -> Line<'static> {
    let is_selected = mode == AppMode::EditingSettings && selected == field;
    let label_style = if is_selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    };

    let shown = if value.trim().is_empty() {
        "(default)"
    } else {
        value
    };
    let value_style = if is_selected {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };

    Line::from(vec![
        Span::styled(format!("{label:<8}"), label_style),
        Span::raw(" "),
        Span::styled(shown.to_string(), value_style),
    ])
}

fn render_help(app: &App) -> Paragraph<'static> {
    let line = match app.mode {
        AppMode::Ready => "n new run | r retry last | s settings | q quit | Ctrl+C force exit",
        AppMode::EditingPrompt => "Enter launch run | Esc cancel | Backspace edit",
        AppMode::EditingSettings => "Up/Down choose field | type edit | Esc close",
        AppMode::Running => "c cancel run | Ctrl+C exit app | q disabled during active run",
    };

    Paragraph::new(vec![Line::from(line)])
        .block(Block::default().title("Controls").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
}

fn selected_setting_mut(settings: &mut RunSettings, field: SettingsField) -> &mut String {
    match field {
        SettingsField::Model => &mut settings.model,
        SettingsField::Sandbox => &mut settings.sandbox,
        SettingsField::Approval => &mut settings.approval,
    }
}

#[derive(Clone, Copy)]
enum LayoutMode {
    Wide,
    Narrow,
}

fn layout_mode(area: Rect) -> LayoutMode {
    if area.width < 140 {
        LayoutMode::Narrow
    } else {
        LayoutMode::Wide
    }
}

fn split_top_middle(area: Rect, mode: LayoutMode, has_controls: bool) -> ([Rect; 2], [Rect; 2]) {
    let base = if has_controls {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Length(9),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(area)
    };

    let top_source = base[0];
    let middle_source = if has_controls { base[2] } else { base[1] };

    let top = match mode {
        LayoutMode::Wide => Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(top_source),
        LayoutMode::Narrow => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Length(3)])
            .split(top_source),
    };

    let middle = match mode {
        LayoutMode::Wide => Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(middle_source),
        LayoutMode::Narrow => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(8)])
            .split(middle_source),
    };

    ([top[0], top[1]], [middle[0], middle[1]])
}

fn controls_area(area: Rect, mode: LayoutMode) -> [Rect; 2] {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Min(10),
            Constraint::Length(5),
        ])
        .split(area);

    let controls = match mode {
        LayoutMode::Wide => Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(vertical[1]),
        LayoutMode::Narrow => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Length(5)])
            .split(vertical[1]),
    };

    [controls[0], controls[1]]
}

fn footer_area(area: Rect, has_controls: bool) -> Rect {
    let vertical = if has_controls {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Length(9),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(area)
    };

    vertical[vertical.len() - 1]
}

fn content_limit(width: u16, reserved: usize) -> usize {
    let width = usize::from(width);
    width.saturating_sub(reserved).max(12)
}

fn compact_timestamp(timestamp: &str) -> String {
    if timestamp.len() <= 8 {
        timestamp.to_string()
    } else if let Some(time_index) = timestamp.find('T') {
        timestamp[time_index + 1..].chars().take(8).collect()
    } else {
        timestamp.chars().take(8).collect()
    }
}

fn short_kind_label(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Status => "STAT",
        EventKind::Command => "CMD",
        EventKind::File => "FILE",
        EventKind::Warning => "WARN",
        EventKind::Error => "ERR",
        EventKind::Note => "NOTE",
    }
}

impl Drop for AppTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            Show,
            MoveTo(0, 0),
            Clear(ClearType::All)
        );
        let _ = self.terminal.show_cursor();
    }
}
