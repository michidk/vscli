use color_eyre::eyre::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use log::debug;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    prelude::{Alignment, Direction, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Padding, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use std::{borrow::Cow, io, rc::Rc};

use crate::history::{Entry, History, Tracker};

/// The UI state
struct UI<'a> {
    state: TableState,
    tracker: &'a mut Tracker,
}

impl<'a> UI<'a> {
    /// Create new empty state from history tracker reference
    fn new(tracker: &'a mut Tracker) -> UI<'a> {
        UI {
            state: TableState::default(),
            tracker,
        }
    }

    /// Select the next entry
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.tracker.history.len().saturating_sub(1) {
                    0
                } else {
                    i.saturating_add(1)
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Select the previous entry
    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tracker.history.len().saturating_sub(1)
                } else {
                    i.saturating_sub(1)
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

/// Starts the UI and returns the selected/resulting entry
pub(crate) fn start(tracker: &mut Tracker) -> Result<Option<Entry>> {
    debug!("Starting UI...");

    // setup terminal
    debug!("Entering raw mode & alternate screen...");
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let res = run_app(&mut terminal, UI::new(tracker));

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    debug!("Terminal restored");

    Ok(res?.and_then(|index| tracker.history.iter().nth(index).cloned()))
}

/// UI main loop
fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: UI) -> io::Result<Option<usize>> {
    app.state.select(Some(0)); // Select the most recent element by default

    let mut rows: Vec<Row> = app
        .tracker
        .history
        .iter()
        .map(|item| {
            let item = item.clone();
            let cells: Vec<String> = vec![
                item.workspace_name.to_string(),
                item.dev_container_name.as_deref().unwrap_or("").to_string(),
                item.workspace_path.to_string_lossy().to_string(),
                item.last_opened
                    .clone()
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            ];
            Row::new(cells).height(1)
        })
        .collect();

    loop {
        terminal.draw(|f| render(f, &mut app, rows.clone()))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => return Ok(None),
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.previous(),
                KeyCode::Enter | KeyCode::Char('o') => {
                    if let Some(selected) = app.state.selected() {
                        return Ok(Some(selected));
                    }
                }
                KeyCode::Delete | KeyCode::Char('r' | 'x') => {
                    if let Some(selected) = app.state.selected() {
                        let entry = app.tracker.history.iter().nth(selected).unwrap().clone();
                        app.tracker.history.remove(&entry);
                        rows.remove(selected);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Renders the main table
fn render_table(
    frame: &mut Frame,
    app: &mut UI,
    rows: Vec<Row>,
    area: Rect,
    longest_ws_name: u16,
    longest_dc_name: u16,
) {
    let selected_style = Style::default().bg(Color::DarkGray);
    let normal_style = Style::default().bg(Color::Blue);
    let header_cells = ["Workspace", "Dev Container", "Path", "Last Opened"]
        .iter()
        .map(|header| Cell::from(*header).style(Style::default().fg(Color::White)));
    let header = Row::new(header_cells).style(normal_style).height(1);

    let widths = [
        Constraint::Min(longest_ws_name + 1),
        Constraint::Min(longest_dc_name + 1),
        Constraint::Percentage(70),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("VSCLI - Recent Workspaces"),
        )
        .highlight_style(selected_style)
        .highlight_symbol("> ");
    frame.render_stateful_widget(table, area, &mut app.state);
}

/// Renders the status area
fn render_status_area(frame: &mut Frame, selected: Option<&Entry>, status_area: &Rc<[Rect]>) {
    let strategy = map_or_default(selected, "-", |entry| entry.behavior.strategy.to_string());
    let insiders = map_or_default(selected, "-", |entry| entry.behavior.insiders.to_string());

    let additional_info = Span::styled(
        format!("Strategy: {strategy}, Insiders: {insiders}"),
        Style::default().fg(Color::DarkGray),
    );

    let status_block = Block::default().padding(Padding::new(2, 2, 0, 0));
    let additional_info_par = Paragraph::new(additional_info)
        .block(status_block.clone())
        .alignment(Alignment::Right);
    frame.render_widget(additional_info_par, status_area[1]);

    let instruction = Span::styled(
        "Press x to remove the selected item. Press q to quit.",
        Style::default().fg(Color::Gray),
    );
    let instructions_par = Paragraph::new(instruction)
        .block(status_block)
        .alignment(Alignment::Left);
    frame.render_widget(instructions_par, status_area[0]);
}

/// Renders additional information like args and dev container path
fn render_additional_info(frame: &mut Frame, selected: Option<&Entry>, area: &Rc<[Rect]>) {
    // Args
    let args_count = map_or_default(selected, "0", |entry| entry.behavior.args.len().to_string());
    let args = selected.map_or(String::from("-"), |entry| {
        let converted_str: Vec<Cow<'_, str>> = entry
            .behavior
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        converted_str.join(", ")
    });

    let args_info = Span::styled(
        format!("Args ({args_count}): {args}"),
        Style::default().fg(Color::DarkGray),
    );
    let args_info_par = Paragraph::new(args_info)
        .block(Block::default().padding(Padding::new(2, 2, 0, 0)))
        .alignment(Alignment::Right);
    frame.render_widget(args_info_par, area[2]);

    // Dev container path
    let dc_path = selected.map_or_else(String::new, |entry| {
        entry
            .config_path
            .as_ref()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let dc_path_info = Span::styled(
        format!("Dev Container: {dc_path}"),
        Style::default().fg(Color::DarkGray),
    );
    let dc_path_info_par = Paragraph::new(dc_path_info).block(Block::default());
    frame.render_widget(dc_path_info_par, area[3]);
}

/// Main render function
fn render(frame: &mut Frame, app: &mut UI, rows: Vec<Row>) {
    // Setup crossterm UI layout & style
    let area = Layout::default()
        .constraints(
            [
                Constraint::Percentage(100),
                Constraint::Min(1),
                Constraint::Min(1),
                Constraint::Min(1),
            ]
            .as_ref(),
        )
        .horizontal_margin(1)
        .split(frame.area());

    // Calculate the longest workspace and dev container names
    let longest_ws_name =
        calculate_longest_name(&app.tracker.history, |s| &s.workspace_name, 20, 9, 60);
    let longest_dc_name = calculate_longest_name(
        &app.tracker.history,
        |s| s.dev_container_name.as_deref().unwrap_or_default(),
        20,
        9,
        60,
    );

    // Render the main table
    render_table(frame, app, rows, area[0], longest_ws_name, longest_dc_name);

    // Get the selected entry
    let selected = app
        .tracker
        .history
        .iter()
        .nth(app.state.selected().unwrap_or(0));

    // Render status area
    let status_area: Rc<[Rect]> = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(area[1]);
    render_status_area(frame, selected, &status_area);

    // Render additional info like args and dev container path
    render_additional_info(frame, selected, &area);
}

/// Calculates the longest name from a history vector
fn calculate_longest_name<'a, F>(
    history: &'a History,
    extractor: F,
    default: u16,
    min: usize,
    max: usize,
) -> u16
where
    F: Fn(&'a Entry) -> &'a str,
{
    u16::try_from(
        history
            .iter()
            .map(|entry| extractor(entry).len())
            .max()
            .unwrap_or(default.into())
            .clamp(min, max),
    )
    .unwrap_or(default)
}
/// Maps an option to a string, using a default value if the option is `None`
fn map_or_default<T, F: Fn(T) -> String>(option: Option<T>, default: &str, f: F) -> String {
    option.map_or(default.to_string(), f)
}
