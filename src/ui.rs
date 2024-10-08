use color_eyre::eyre::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
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
use tui_textarea::{Input, Key, TextArea};
use uuid::Uuid;

use crate::history::{Entry, History, Tracker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Search,
    Select,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppAction {
    Quit,
    Selected(Uuid),
    DeleteEntry(Uuid),
    SearchUpdate(Option<String>),
}

struct TableRow {
    entry: Entry,
    row: Row<'static>,
    search_score: Option<i64>,
}

impl From<Entry> for TableRow {
    fn from(value: Entry) -> Self {
        let cells: Vec<String> = vec![
            value.workspace_name.to_string(),
            value
                .dev_container_name
                .as_deref()
                .unwrap_or("")
                .to_string(),
            value.workspace_path.to_string_lossy().to_string(),
            value
                .last_opened
                .clone()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ];
        let row = Row::new(cells).height(1);

        Self {
            row,
            entry: value,
            search_score: Some(0),
        }
    }
}

struct TableData {
    rows: Vec<TableRow>,

    max_worspace_name_len: Option<usize>,
    max_devcontainer_name_len: Option<usize>,
}

impl TableData {
    pub const HEADER: [&'static str; 4] = ["Workspace", "Dev Container", "Path", "Last Opened"];

    pub fn from_iter<I: Iterator<Item = E>, E: Into<Entry>>(iter: I) -> Self {
        let mut this = Self {
            rows: iter.into_iter().map(|entry| entry.into().into()).collect(),

            max_devcontainer_name_len: None,
            max_worspace_name_len: None,
        };

        // Sort by `Last Opened` to keep same logic as previous versions
        // Inverted to have newest at the top with ASC order
        this.rows
            .sort_by_key(|entry| -entry.entry.last_opened.timestamp());

        this.recalculate_maxes();

        this
    }

    pub fn is_filtered_out(&self, uuid: Uuid) -> Option<bool> {
        self.rows
            .iter()
            .find(|row| row.entry.uuid == uuid)
            .map(|entry| entry.search_score.is_none())
    }

    fn recalculate_maxes(&mut self) {
        self.max_worspace_name_len = self
            .rows
            .iter()
            .map(|row| row.entry.workspace_name.len())
            .max();

        self.max_devcontainer_name_len = self
            .rows
            .iter()
            .map(|row| row.entry.dev_container_name.as_deref().unwrap_or("").len())
            .max();
    }

    pub fn to_rows(&self) -> Vec<Row<'static>> {
        self.rows
            .iter()
            .filter(|row| row.search_score.is_some())
            .map(|row| &row.row)
            .cloned()
            .collect()
    }

    pub fn apply_filter(&mut self, pattern: &str) -> bool {
        let mut changes = false;

        let matcher = SkimMatcherV2::default();

        for row in &mut self.rows {
            let new_search_score = add_num_opt(
                add_num_opt(
                    matcher.fuzzy_match(&row.entry.workspace_name, pattern),
                    matcher.fuzzy_match(
                        row.entry.dev_container_name.as_deref().unwrap_or(""),
                        pattern,
                    ),
                ),
                matcher.fuzzy_match(&row.entry.workspace_path.to_string_lossy(), pattern),
            );
            changes |= new_search_score != row.search_score;
            row.search_score = new_search_score;
        }

        self.rows
            .sort_by_key(|row| row.search_score.unwrap_or(i64::MIN));

        return changes;
    }

    pub fn reset_filter(&mut self) {
        for row in &mut self.rows {
            row.search_score = Some(0);
        }

        // Sort by `Last Opened` to keep same logic as previous versions
        // Inverted to have newest at the top with ASC order
        self.rows
            .sort_by_key(|entry| -entry.entry.last_opened.timestamp());
    }
}

/// The UI state
struct UI<'a> {
    mode: Mode,
    search: TextArea<'a>,
    table_state: TableState,
    table_data: TableData,
}

impl<'a> UI<'a> {
    /// Create new empty state from history tracker reference
    pub fn new(history: &History) -> UI<'a> {
        UI {
            // TODO: Add option for initial mode
            mode: Mode::Search,
            search: TextArea::default(),
            table_state: TableState::default(),
            table_data: TableData::from_iter(history.iter().cloned()),
        }
    }

    /// Select the next entry
    pub fn select_next(&mut self) {
        self.table_state.select_next();
    }

    /// Select the previous entry
    pub fn select_previous(&mut self) {
        self.table_state.select_previous();
    }

    pub fn apply_filter(&mut self, pattern: Option<&str>) {
        let pattern = pattern.unwrap_or("");

        let prev_selected = self.get_selected_entry().cloned();

        let update_selected = if pattern.trim().is_empty() {
            self.reset_filter();
            true
        } else {
            self.table_data.apply_filter(pattern)
        };

        if !update_selected {
            return;
        }

        // TODO: This logic is still not working as intended. Also cleanup
        // See if selected item is still visible. If not select first, else reselect (index changed)
        if let Some(selected) = prev_selected {
            if self
                .table_data
                .is_filtered_out(selected.uuid)
                .unwrap_or(true)
            {
                self.table_state.select_first();
            } else {
                // Update index
                if let Some(index) = self
                    .table_data
                    .rows
                    .iter()
                    .filter(|entry| {
                        !self
                            .table_data
                            .is_filtered_out(entry.entry.uuid)
                            .unwrap_or(true)
                    })
                    .position(|entry| entry.entry.uuid == selected.uuid)
                {
                    self.table_state.select(Some(index));
                } else {
                    self.table_state.select_first();
                }
            }
        }
    }

    pub fn reset_filter(&mut self) {
        self.table_data.reset_filter();
    }

    fn get_selected_entry(&self) -> Option<&Entry> {
        let idx = self.table_state.selected()?;
        self.table_data.rows.get(idx).map(|row| &row.entry)
    }

    fn delete_by_uuid(&mut self, uuid: Uuid) -> bool {
        if let Some(index) = self
            .table_data
            .rows
            .iter()
            .position(|entry| entry.entry.uuid == uuid)
        {
            self.table_data.rows.remove(index);
            return true;
        }

        false
    }

    fn reset_selected(&mut self) {
        self.table_state.select(Some(0));
    }

    fn resync_table(&mut self, history: &History) {
        self.table_data = TableData::from_iter(history.iter().cloned());
        self.reset_selected();
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
    let res = run_app(&mut terminal, UI::new(&tracker.history), tracker);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    debug!("Terminal restored");

    Ok(res?.and_then(|uuid| {
        tracker
            .history
            .iter()
            .find(|entry| entry.uuid == uuid)
            .cloned()
    }))
}

/// UI main loop
fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: UI,
    tracker: &mut Tracker,
) -> io::Result<Option<Uuid>> {
    app.table_state.select(Some(0)); // Select the most recent element by default

    loop {
        terminal.draw(|f| render(f, &mut app))?;

        let input = event::read()?;

        let res = match app.mode {
            Mode::Search => handle_input_search(&mut app, input),
            Mode::Select => handle_input_select(&mut app, input),
        }?;

        if let Some(res) = res {
            match res {
                AppAction::Quit => return Ok(None),
                AppAction::Selected(selected) => return Ok(Some(selected)),
                AppAction::DeleteEntry(uuid) => {
                    if tracker.history.remove_by_uuid(uuid) {
                        if !app.delete_by_uuid(uuid) {
                            // Desync - Deleted from history but not from UI.
                            app.resync_table(&tracker.history);
                        }
                    }
                }
                AppAction::SearchUpdate(pattern) => {
                    app.apply_filter(pattern.as_deref());
                }
            }
        }
    }
}

fn handle_input_select(app: &mut UI, input: Event) -> io::Result<Option<AppAction>> {
    if let Event::Key(key) = input {
        if key.kind != KeyEventKind::Press {
            return Ok(None);
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => {
                return Ok(Some(AppAction::Quit))
            }
            KeyCode::Down | KeyCode::Char('j') => app.select_next(),
            KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
            KeyCode::Enter | KeyCode::Char('o') => {
                if let Some(selected) = app.get_selected_entry() {
                    return Ok(Some(AppAction::Selected(selected.uuid)));
                }
            }
            KeyCode::Delete | KeyCode::Char('r' | 'x') => {
                if let Some(selected) = app.get_selected_entry() {
                    return Ok(Some(AppAction::DeleteEntry(selected.uuid)));
                }
            }
            KeyCode::Tab => {
                app.mode = Mode::Search;
            }
            _ => {}
        }
    }

    Ok(None)
}

fn handle_input_search(app: &mut UI, input: Event) -> io::Result<Option<AppAction>> {
    match input.into() {
        Input { key: Key::Esc, .. }
        | Input { key: Key::Tab, .. }
        | Input {
            key: Key::Enter, ..
        } => {
            app.mode = Mode::Select;
        }
        input => {
            if app.search.input(input) {
                return Ok(Some(AppAction::SearchUpdate(
                    app.search.lines().first().cloned(),
                )));
            }
        }
    }

    Ok(None)
}

/// Main render function
fn render(frame: &mut Frame, app: &mut UI) {
    // Setup crossterm UI layout & style
    let area = Layout::default()
        .constraints(
            [
                Constraint::Min(3),
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
    let longest_ws_name = app
        .table_data
        .max_worspace_name_len
        .unwrap_or(20)
        .clamp(9, 60);

    let longest_dc_name = app
        .table_data
        .max_devcontainer_name_len
        .unwrap_or(20)
        .clamp(9, 60);

    render_search_input(frame, app, area[0]);

    // Render the main table
    render_table(
        frame,
        app,
        area[1],
        longest_ws_name as u16,
        longest_dc_name as u16,
    );

    let selected: Option<&Entry> = app.get_selected_entry();

    // Render status area
    let status_area: Rc<[Rect]> = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(area[2]);
    render_status_area(frame, selected, &status_area);

    // Render additional info like args and dev container path
    render_additional_info(frame, selected, [area[3], area[4]]);
}

fn render_search_input(frame: &mut Frame, app: &mut UI, area: Rect) {
    let style = if app.mode == Mode::Search {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    app.search.set_block(
        Block::default()
            .borders(Borders::all())
            .title("Search")
            .border_style(style),
    );

    frame.render_widget(&app.search, area);
}

/// Renders the main table
fn render_table(
    frame: &mut Frame,
    app: &mut UI,
    area: Rect,
    longest_ws_name: u16,
    longest_dc_name: u16,
) {
    let (header_style, selected_style) = if app.mode == Mode::Select {
        (
            Style::default().bg(Color::Blue),
            Style::default().bg(Color::DarkGray),
        )
    } else {
        (
            Style::default().bg(Color::DarkGray),
            Style::default().bg(Color::DarkGray),
        )
    };

    let header_cells = TableData::HEADER
        .iter()
        .map(|header| Cell::from(*header).style(Style::default().fg(Color::White)));
    let header = Row::new(header_cells).style(header_style).height(1);

    let widths = [
        Constraint::Min(longest_ws_name + 1),
        Constraint::Min(longest_dc_name + 1),
        Constraint::Percentage(70),
        Constraint::Min(20),
    ];

    let table = Table::new(app.table_data.to_rows(), widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("VSCLI - Recent Workspaces"),
        )
        .highlight_style(selected_style)
        .highlight_symbol("> ");
    frame.render_stateful_widget(table, area, &mut app.table_state);
}

/// Renders the status area
fn render_status_area(frame: &mut Frame, selected: Option<&Entry>, status_area: &Rc<[Rect]>) {
    let strategy = selected.map_or_else(
        || String::from("-"),
        |entry| entry.behavior.strategy.to_string(),
    );

    let insiders = selected.map_or_else(
        || String::from("-"),
        |entry| entry.behavior.insiders.to_string(),
    );

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
fn render_additional_info(frame: &mut Frame, selected: Option<&Entry>, area: [Rect; 2]) {
    // Args

    let args_count = selected.map_or(0, |entry| entry.behavior.args.len());

    let args = selected.map_or_else(
        || String::from("-"),
        |entry| {
            let converted_str: Vec<Cow<'_, str>> = entry
                .behavior
                .args
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect();
            converted_str.join(", ")
        },
    );

    let args_info = Span::styled(
        format!("Args ({args_count}): {args}"),
        Style::default().fg(Color::DarkGray),
    );
    let args_info_par = Paragraph::new(args_info)
        .block(Block::default().padding(Padding::new(2, 2, 0, 0)))
        .alignment(Alignment::Right);
    frame.render_widget(args_info_par, area[0]);

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
    frame.render_widget(dc_path_info_par, area[1]);
}

fn add_num_opt(o1: Option<i64>, o2: Option<i64>) -> Option<i64> {
    match (o1, o2) {
        (Some(n1), Some(n2)) => Some(n1 + n2),
        (Some(n), None) | (None, Some(n)) => Some(n),
        _ => None,
    }
}
