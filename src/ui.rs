use chrono::{DateTime, Local};
use color_eyre::eyre::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use log::debug;
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    prelude::{Alignment, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{
        Block, Borders, Cell, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
};
use std::{borrow::Cow, io};
use tui_textarea::TextArea;

use crate::history::{Entry, EntryId, History, Tracker};

/// All "user triggered" action which the app might want to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AppAction {
    Quit,
    SelectNext,
    SelectPrevious,
    SelectFirst,
    SelectLast,
    OpenSelected,
    DeleteSelectedEntry,
    SearchInput(tui_textarea::Input),
    TableClick(u16), // New variant for table clicks with row position
}

/// Represents a single record/entry of the UI table.
///
/// Additional to the representation ([`Self::row`]) it also contains other meta information to
/// make e.g. filtering possible and efficient.
#[derive(Debug, Clone)]
struct TableRow {
    id: EntryId,
    entry: Entry,
    row: Row<'static>,
    search_score: Option<u32>,
}

impl From<(EntryId, Entry)> for TableRow {
    fn from((id, value): (EntryId, Entry)) -> Self {
        let cells: Vec<String> = vec![
            value.workspace_name.to_string(),
            value
                .dev_container_name
                .as_deref()
                .unwrap_or("")
                .to_string(),
            value.workspace_path.to_string_lossy().to_string(),
            DateTime::<Local>::from(value.last_opened)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ];
        let row = Row::new(cells).height(1);

        Self {
            id,
            row,
            entry: value,
            search_score: Some(0),
        }
    }
}

/// Contains all UI related elements to display and operate on the entries of the table.
#[derive(Debug, Clone)]
struct TableData {
    /// Be very careful when accessing this value directly as it represents all values regardless of
    /// applied filter or not. It only makes sense if the action does not care about the filter and
    /// if entries are accessed by id and not index/position.
    ///
    /// Most of the times [`Self::to_rows`] or [`Self::as_rows_full`] are desired.
    rows: Vec<TableRow>,

    /// Caches the longest workspace name [`Self::rows`] contains.
    ///
    /// Note that this value does not change for a "session" even if a filter is applied and/or the
    /// longest entry is deleted.
    max_worspace_name_len: Option<usize>,

    /// Caches the longest devcontainer name [`Self::rows`] contains.
    ///
    /// Note that this value does not change for a "session" even if a filter is applied and/or the
    /// longest entry is deleted.
    max_devcontainer_name_len: Option<usize>,
}

impl TableData {
    pub const HEADER: [&'static str; 4] = ["Workspace", "Dev Container", "Path", "Last Opened"];

    pub fn from_iter<I: Iterator<Item = (EntryId, Entry)>>(iter: I) -> Self {
        let mut this = Self {
            rows: iter.into_iter().map(TableRow::from).collect(),

            max_devcontainer_name_len: None,
            max_worspace_name_len: None,
        };

        // Sort by `Last Opened` to keep same logic as previous versions
        // Inverted to have newest at the top with ASC order
        this.rows
            .sort_by_key(|entry| -entry.entry.last_opened.timestamp());

        this.max_worspace_name_len = this
            .rows
            .iter()
            .map(|row| row.entry.workspace_name.len())
            .max();

        this.max_devcontainer_name_len = this
            .rows
            .iter()
            .map(|row| row.entry.dev_container_name.as_deref().unwrap_or("").len())
            .max();

        this
    }

    pub fn to_rows(&self) -> Vec<Row<'static>> {
        self.rows
            .iter()
            .filter(|row| row.search_score.is_some())
            .map(|row| &row.row)
            .cloned()
            .collect()
    }

    pub fn as_rows_full(&self) -> impl Iterator<Item = &TableRow> {
        self.rows.iter().filter(|row| row.search_score.is_some())
    }

    pub fn apply_filter(&mut self, pattern: &str) -> bool {
        let mut changes = false;
        let mut matcher = Matcher::default();
        let mut buf = Vec::new();

        let pattern = Pattern::new(
            pattern,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );

        for row in &mut self.rows {
            let workspace_name = row.entry.workspace_name.as_str();
            let container_name = row.entry.dev_container_name.as_deref().unwrap_or("");
            let path_str = row.entry.workspace_path.to_string_lossy();

            let new_search_score = add_num_opt(
                add_num_opt(
                    pattern.score(Utf32Str::new(workspace_name, &mut buf), &mut matcher),
                    pattern.score(Utf32Str::new(container_name, &mut buf), &mut matcher),
                ),
                pattern.score(Utf32Str::new(path_str.as_ref(), &mut buf), &mut matcher),
            );
            changes |= new_search_score != row.search_score;
            row.search_score = new_search_score;
        }

        self.rows
            .sort_by_key(|row| u32::MAX - row.search_score.unwrap_or(0));

        changes
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
    search: TextArea<'a>,
    table_state: TableState,
    table_data: TableData,
    hide_instructions: bool,
    hide_info: bool,
    last_clicked_index: Option<usize>, // Track the last clicked row
}

impl<'a> UI<'a> {
    /// Create new empty state from history tracker reference
    pub fn new(history: &History, hide_instructions: bool, hide_info: bool) -> UI<'a> {
        UI {
            search: TextArea::default(),
            table_state: TableState::default(),
            table_data: TableData::from_iter(
                history.iter().map(|(id, entry)| (*id, entry.clone())),
            ),
            hide_instructions,
            hide_info,
            last_clicked_index: None,
        }
    }

    /// Select the next entry with wrapping
    pub fn select_next(&mut self) {
        let len = self.table_data.as_rows_full().count();
        if len == 0 {
            return;
        }

        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some((i + 1) % len));
    }

    /// Select the previous entry with wrapping
    pub fn select_previous(&mut self) {
        let len = self.table_data.as_rows_full().count();
        if len == 0 {
            return;
        }

        let i = self.table_state.selected().unwrap_or(len - 1);
        self.table_state.select(Some((i + len - 1) % len));
    }

    pub fn select_first(&mut self) {
        self.table_state.select_first();
    }

    pub fn select_last(&mut self) {
        self.table_state.select_last();
    }

    pub fn apply_filter(&mut self, pattern: Option<&str>) {
        let pattern = pattern.unwrap_or("");

        let prev_selected = self.get_selected_row();

        let update_selected = if pattern.trim().is_empty() {
            self.reset_filter();
            true
        } else {
            self.table_data.apply_filter(pattern)
        };

        if !update_selected {
            return;
        }

        // See if selected item is still visible. If not select first, else reselect (index changed)
        if let Some(selected) = prev_selected {
            let new_rows = self.table_data.as_rows_full();

            match new_rows
                .enumerate()
                .find_map(|(index, entry)| (entry.id == selected.id).then_some(index))
            {
                Some(index) => {
                    // Update index
                    self.table_state.select(Some(index));
                }
                _ => {
                    self.table_state.select_first();
                }
            }
        } else {
            self.table_state.select_first();
        }
    }

    pub fn reset_filter(&mut self) {
        self.table_data.reset_filter();
    }

    fn get_selected_row(&self) -> Option<TableRow> {
        let index = self.table_state.selected()?;
        self.table_data.as_rows_full().nth(index).cloned()
    }

    fn delete(&mut self, entry_id: EntryId) -> bool {
        if let Some(index) = self
            .table_data
            .rows
            .iter()
            .position(|entry| entry.id == entry_id)
        {
            self.table_data.rows.remove(index);
            return true;
        }

        false
    }

    fn reset_selected(&mut self) {
        self.table_state.select(Some(0));
    }

    /// Replaces the previous [`Self::table_data`] with a newly calculated one.
    ///
    /// This should only be done if there is a "desync" issue (e.g. deleted from history but failed
    /// to delete from table data).
    fn resync_table(&mut self, history: &History) {
        self.table_data =
            TableData::from_iter(history.iter().map(|(id, entry)| (*id, entry.clone())));
        self.reset_selected();
    }
}

/// Starts the UI and returns the selected/resulting entry
pub(crate) fn start(
    tracker: &mut Tracker,
    hide_instructions: bool,
    hide_info: bool,
) -> Result<Option<(EntryId, Entry)>> {
    debug!("Starting UI...");

    // setup terminal
    debug!("Entering raw mode & alternate screen...");
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let res = run_app(
        &mut terminal,
        UI::new(&tracker.history, hide_instructions, hide_info),
        tracker,
    );

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    debug!("Terminal restored");

    Ok(res?.and_then(|selected_id| {
        tracker
            .history
            .iter()
            .find(|(id, _)| **id == selected_id)
            .map(|(id, entry)| (*id, entry.clone()))
    }))
}

/// UI main loop
fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: UI,
    tracker: &mut Tracker,
) -> io::Result<Option<EntryId>> {
    app.table_state.select(Some(0)); // Select the most recent element by default

    loop {
        terminal.draw(|f| render(f, &mut app))?;

        let input = event::read()?;
        let action = handle_input(input);

        if let Some(action) = action {
            match action {
                AppAction::Quit => return Ok(None),
                AppAction::SelectNext => {
                    app.select_next();
                    app.last_clicked_index = None; // Reset click tracking on navigation
                }
                AppAction::SelectPrevious => {
                    app.select_previous();
                    app.last_clicked_index = None; // Reset click tracking on navigation
                }
                AppAction::SelectFirst => {
                    app.select_first();
                    app.last_clicked_index = None; // Reset click tracking on navigation
                }
                AppAction::SelectLast => {
                    app.select_last();
                    app.last_clicked_index = None; // Reset click tracking on navigation
                }
                AppAction::OpenSelected => {
                    if let Some(selected) = app.get_selected_row() {
                        return Ok(Some(selected.id));
                    }
                }
                AppAction::DeleteSelectedEntry => {
                    if let Some(selected) = app.get_selected_row() {
                        let entry_id = selected.id;
                        if tracker.history.delete(entry_id).is_some() && !app.delete(entry_id) {
                            app.resync_table(&tracker.history);
                        }
                    }
                    app.last_clicked_index = None; // Reset click tracking after deletion
                }
                AppAction::TableClick(row) => {
                    // Check if click is within table area (accounting for borders and header)
                    let table_area = terminal.get_frame().area();
                    if row >= table_area.y + 2 && row < table_area.y + table_area.height - 1 {
                        let clicked_index = (row - table_area.y - 2) as usize;
                        let visible_rows = app.table_data.as_rows_full().count();

                        if clicked_index < visible_rows {
                            // If clicking the same row that was previously clicked and selected
                            if app.last_clicked_index == Some(clicked_index)
                                && app.table_state.selected() == Some(clicked_index)
                            {
                                // Launch the container
                                if let Some(selected) = app.get_selected_row() {
                                    return Ok(Some(selected.id));
                                }
                            } else {
                                // Just select the row on first click
                                app.table_state.select(Some(clicked_index));
                                app.last_clicked_index = Some(clicked_index);
                            }
                        }
                    }
                }
                AppAction::SearchInput(input) => {
                    if app.search.input(input) {
                        let line = app.search.lines().first().cloned();
                        app.apply_filter(line.as_deref());
                        app.last_clicked_index = None; // Reset click tracking on search
                    }
                }
            }
        }
    }
}

fn handle_input(input: Event) -> Option<AppAction> {
    match input {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return None;
            }

            let is_key = |code: KeyCode| key.code == code;
            let is_char = |c: char| is_key(KeyCode::Char(c));
            let is_ctrl_char =
                |c: char| key.modifiers.contains(KeyModifiers::CONTROL) && is_char(c);

            if is_key(KeyCode::Esc) || is_ctrl_char('q') || is_ctrl_char('c') {
                return Some(AppAction::Quit);
            } else if is_key(KeyCode::Down) || is_ctrl_char('j') {
                return Some(AppAction::SelectNext);
            } else if is_key(KeyCode::Up) || is_ctrl_char('k') {
                return Some(AppAction::SelectPrevious);
            } else if is_key(KeyCode::KeypadBegin) || is_ctrl_char('1') {
                return Some(AppAction::SelectFirst);
            } else if is_key(KeyCode::End) || is_ctrl_char('0') {
                return Some(AppAction::SelectLast);
            } else if is_key(KeyCode::Enter) || is_ctrl_char('o') {
                return Some(AppAction::OpenSelected);
            } else if is_key(KeyCode::Delete) || is_ctrl_char('r') || is_ctrl_char('x') {
                return Some(AppAction::DeleteSelectedEntry);
            }
        }
        Event::Mouse(MouseEvent { kind, row, .. }) => match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                return Some(AppAction::TableClick(row));
            }
            MouseEventKind::ScrollDown => {
                return Some(AppAction::SelectNext);
            }
            MouseEventKind::ScrollUp => {
                return Some(AppAction::SelectPrevious);
            }
            _ => {}
        },
        _ => {}
    }

    Some(AppAction::SearchInput(input.into()))
}

/// Main render function
fn render(frame: &mut Frame, app: &mut UI) {
    // Setup crossterm UI layout & style
    let constraints = if app.hide_info {
        vec![
            Constraint::Percentage(100),
            Constraint::Min(3),
            Constraint::Min(1),
        ]
    } else {
        vec![
            Constraint::Percentage(100),
            Constraint::Min(3),
            Constraint::Min(1),
            Constraint::Min(1),
            Constraint::Min(1),
        ]
    };

    let area = Layout::default()
        .constraints(&constraints)
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

    // Render the main table
    render_table(
        frame,
        app,
        area[0],
        u16::try_from(longest_ws_name).unwrap_or(u16::MAX),
        u16::try_from(longest_dc_name).unwrap_or(u16::MAX),
    );

    render_search_input(frame, app, area[1]);

    let selected: Option<Entry> = app.get_selected_row().map(|row| row.entry);

    // Render status area and additional info
    render_status_area(
        frame,
        selected.as_ref(),
        &area[2..],
        app.hide_instructions,
        app.hide_info,
    );
}

fn render_search_input(frame: &mut Frame, app: &mut UI, area: Rect) {
    let style = Style::default().fg(Color::Blue);

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
    let (header_style, selected_style) = (
        Style::default().bg(Color::Blue),
        Style::default().bg(Color::DarkGray),
    );

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
                .title("Recent Workspaces"),
        )
        .row_highlight_style(selected_style)
        .highlight_symbol("> ");
    frame.render_stateful_widget(table, area, &mut app.table_state);

    // Calculate if scrollbar is needed
    let total_items = app.table_data.as_rows_full().count();
    let viewport_height = (area.height - 2) as usize; // Subtract 2 for borders

    // Show scrollbar if there's any content not visible in the viewport
    if total_items >= viewport_height {
        let mut scrollbar_state = ScrollbarState::default()
            .content_length(total_items)
            .viewport_content_length(viewport_height)
            .position(app.table_state.selected().unwrap_or(0));

        // Create a new area for the scrollbar that overlaps with the right border
        let scrollbar_area = Rect {
            x: area.x + area.width - 1, // Place on the right border
            y: area.y + 2,              // Start two lines below the top (one line after header)
            width: 1,
            height: area.height - 3, // Account for top border + header and bottom border
        };

        // Render scrollbar
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓")),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }
}

/// Renders the status area and additional info
fn render_status_area(
    frame: &mut Frame,
    selected: Option<&Entry>,
    areas: &[Rect],
    hide_instructions: bool,
    hide_info: bool,
) {
    // Render instructions using full width if not hidden
    if !hide_instructions {
        let instruction = Span::styled(
            "↑/↓ to navigate • Del/Ctrl+X to remove • Enter to open • Type to filter • Esc/Ctrl+C to quit",
            Style::default().fg(Color::Gray),
        );
        let instructions_par = Paragraph::new(instruction)
            .block(Block::default().padding(Padding::new(2, 2, 0, 0)))
            .alignment(Alignment::Left);
        frame.render_widget(instructions_par, areas[0]);
    }

    // Render additional info if not hidden and we have more areas
    if !hide_info && areas.len() > 1 {
        // Strategy, command and args info
        let strategy = selected.map_or_else(
            || String::from("-"),
            |entry| entry.behavior.strategy.to_string(),
        );

        let command =
            selected.map_or_else(|| String::from("-"), |entry| entry.behavior.command.clone());

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

        let additional_info = Span::styled(
            format!("Strategy: {strategy} • Command: {command} • Args ({args_count}): {args}"),
            Style::default().fg(Color::DarkGray),
        );

        let status_block = Block::default().padding(Padding::new(2, 2, 0, 0));
        let additional_info_par = Paragraph::new(additional_info)
            .block(status_block)
            .alignment(Alignment::Left);
        frame.render_widget(additional_info_par, areas[1]);

        // Dev container path
        if areas.len() > 2 {
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
            let dc_path_info_par = Paragraph::new(dc_path_info)
                .block(Block::default().padding(Padding::new(2, 2, 0, 0)))
                .alignment(Alignment::Left);

            frame.render_widget(dc_path_info_par, areas[2]);
        }
    }
}

/// Adds two optional [`u32`]s.
///
/// If at least one of the inputs is [`Option::Some`] then the result will also be [`Option::Some`].
fn add_num_opt(o1: Option<u32>, o2: Option<u32>) -> Option<u32> {
    match (o1, o2) {
        (Some(n1), Some(n2)) => Some(n1 + n2),
        (Some(n), None) | (None, Some(n)) => Some(n),
        _ => None,
    }
}
