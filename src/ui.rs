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
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    prelude::{Alignment, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{
        Block, Borders, Cell, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
};
use ratatui_textarea::TextArea;
use std::{borrow::Cow, io};

use crate::history::{Entry, EntryId, History, Tracker};

/// Describes an item that can be rendered and filtered by the generic picker UI.
pub trait Pickable: Clone {
    /// Title displayed on top of the table.
    fn title() -> &'static str;

    /// Header labels for the table columns.
    fn headers() -> &'static [&'static str];

    /// Table cells representing this item.
    fn cells(&self) -> Vec<String>;

    /// Fields used by fuzzy search.
    fn search_fields(&self) -> Vec<String>;

    /// Status lines shown below the table.
    fn status_lines(&self) -> Vec<String>;

    /// Table column constraints based on computed maximum cell widths.
    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint>;
}

/// Configuration options for the generic picker UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct PickerOpts {
    /// Hide the instruction line.
    pub hide_instructions: bool,
    /// Hide additional status/info lines.
    pub hide_info: bool,
}

/// Item wrapper for the recent-workspaces picker.
#[derive(Debug, Clone)]
pub struct HistoryItem {
    /// Unique history entry id.
    pub id: EntryId,
    /// Stored history entry.
    pub entry: Entry,
}

impl Pickable for HistoryItem {
    fn title() -> &'static str {
        "Recent Workspaces"
    }

    fn headers() -> &'static [&'static str] {
        &[
            "Workspace",
            "Dev Container",
            "Config",
            "Path",
            "Last Opened",
        ]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.entry.workspace_name.clone(),
            self.entry
                .dev_container_name
                .as_deref()
                .unwrap_or("")
                .to_string(),
            self.entry.config_name.as_deref().unwrap_or("").to_string(),
            self.entry.workspace_path.to_string_lossy().to_string(),
            DateTime::<Local>::from(self.entry.last_opened)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ]
    }

    fn search_fields(&self) -> Vec<String> {
        vec![
            self.entry.workspace_name.clone(),
            self.entry.dev_container_name.clone().unwrap_or_default(),
            self.entry.config_name.clone().unwrap_or_default(),
            self.entry.workspace_path.to_string_lossy().to_string(),
        ]
    }

    fn status_lines(&self) -> Vec<String> {
        let args_count = self.entry.behavior.args.len();
        let args_joined = self
            .entry
            .behavior
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<Cow<'_, str>>>()
            .join(", ");

        let config_path = self
            .entry
            .config_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();

        vec![
            format!(
                "Strategy: {} • Command: {} • Args ({args_count}): {args_joined}",
                self.entry.behavior.strategy, self.entry.behavior.command,
            ),
            format!("Dev Container: {config_path}"),
        ]
    }

    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let workspace_width = max_widths.first().copied().unwrap_or(20).clamp(9, 60);
        let devcontainer_width = max_widths.get(1).copied().unwrap_or(20).clamp(9, 60);
        let config_width = max_widths.get(2).copied().unwrap_or(6).clamp(6, 40);

        vec![
            Constraint::Min(u16::try_from(workspace_width).unwrap_or(u16::MAX)),
            Constraint::Min(u16::try_from(devcontainer_width).unwrap_or(u16::MAX)),
            Constraint::Min(u16::try_from(config_width).unwrap_or(u16::MAX)),
            Constraint::Percentage(70),
            Constraint::Min(20),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppAction {
    Quit,
    SelectNext,
    SelectPrevious,
    SelectFirst,
    SelectLast,
    OpenSelected,
    DeleteSelectedEntry,
    SearchInput(ratatui_textarea::Input),
    TableClick(u16),
}

#[derive(Debug, Clone)]
struct PickerRow<T: Pickable> {
    item: T,
    row: Row<'static>,
    search_score: Option<u32>,
    original_index: usize,
}

#[derive(Debug, Clone)]
struct PickerData<T: Pickable> {
    rows: Vec<PickerRow<T>>,
    max_column_widths: Vec<usize>,
}

impl<T: Pickable> PickerData<T> {
    fn from_items(items: Vec<T>) -> Self {
        let mut rows = Vec::with_capacity(items.len());
        let mut max_column_widths = vec![0; T::headers().len()];

        for (index, item) in items.into_iter().enumerate() {
            let cells = item.cells();

            if cells.len() > max_column_widths.len() {
                max_column_widths.resize(cells.len(), 0);
            }

            for (column, cell) in cells.iter().enumerate() {
                max_column_widths[column] = max_column_widths[column].max(cell.len());
            }

            rows.push(PickerRow {
                item,
                row: Row::new(cells).height(1),
                search_score: Some(0),
                original_index: index,
            });
        }

        Self {
            rows,
            max_column_widths,
        }
    }

    fn to_rows(&self) -> Vec<Row<'static>> {
        self.rows
            .iter()
            .filter(|row| row.search_score.is_some())
            .map(|row| row.row.clone())
            .collect()
    }

    fn as_rows_full(&self) -> impl Iterator<Item = &PickerRow<T>> {
        self.rows.iter().filter(|row| row.search_score.is_some())
    }

    fn apply_filter(&mut self, pattern: &str) -> bool {
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
            let mut score = None;
            for field in row.item.search_fields() {
                score = add_num_opt(
                    score,
                    pattern.score(Utf32Str::new(&field, &mut buf), &mut matcher),
                );
            }

            changes |= score != row.search_score;
            row.search_score = score;
        }

        self.rows
            .sort_by_key(|row| u32::MAX - row.search_score.unwrap_or(0));

        changes
    }

    fn reset_filter(&mut self) {
        for row in &mut self.rows {
            row.search_score = Some(0);
        }

        self.rows.sort_by_key(|row| row.original_index);
    }

    fn delete_by_original_index(&mut self, original_index: usize) -> bool {
        if let Some(index) = self
            .rows
            .iter()
            .position(|entry| entry.original_index == original_index)
        {
            self.rows.remove(index);
            return true;
        }

        false
    }
}

struct PickerState<'a, T: Pickable> {
    search: TextArea<'a>,
    table_state: TableState,
    table_data: PickerData<T>,
    opts: PickerOpts,
    last_clicked_index: Option<usize>,
}

impl<T: Pickable> PickerState<'_, T> {
    fn new(items: Vec<T>, opts: PickerOpts) -> Self {
        Self {
            search: TextArea::default(),
            table_state: TableState::default(),
            table_data: PickerData::from_items(items),
            opts,
            last_clicked_index: None,
        }
    }

    fn select_next(&mut self) {
        let len = self.table_data.as_rows_full().count();
        if len == 0 {
            return;
        }

        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some((i + 1) % len));
    }

    fn select_previous(&mut self) {
        let len = self.table_data.as_rows_full().count();
        if len == 0 {
            return;
        }

        let i = self.table_state.selected().unwrap_or(len - 1);
        self.table_state.select(Some((i + len - 1) % len));
    }

    fn select_first(&mut self) {
        self.table_state.select_first();
    }

    fn select_last(&mut self) {
        self.table_state.select_last();
    }

    fn apply_filter(&mut self, pattern: Option<&str>) {
        let pattern = pattern.unwrap_or("");

        let prev_selected = self.get_selected_row().map(|row| row.original_index);

        let update_selected = if pattern.trim().is_empty() {
            self.table_data.reset_filter();
            true
        } else {
            self.table_data.apply_filter(pattern)
        };

        if !update_selected {
            return;
        }

        if let Some(selected) = prev_selected {
            let new_rows = self.table_data.as_rows_full();

            match new_rows
                .enumerate()
                .find_map(|(index, entry)| (entry.original_index == selected).then_some(index))
            {
                Some(index) => {
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

    fn get_selected_row(&self) -> Option<PickerRow<T>> {
        let index = self.table_state.selected()?;
        self.table_data.as_rows_full().nth(index).cloned()
    }

    fn delete(&mut self, original_index: usize) -> bool {
        self.table_data.delete_by_original_index(original_index)
    }
}

/// Starts a generic picker and returns the selected item.
///
/// # Errors
///
/// Returns an error if terminal setup/teardown fails or if input/rendering fails.
pub fn pick<T: Pickable>(
    items: Vec<T>,
    opts: PickerOpts,
    on_delete: Option<&mut dyn FnMut(&T)>,
) -> Result<Option<T>> {
    debug!("Starting UI...");

    debug!("Entering raw mode & alternate screen...");
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, PickerState::new(items, opts), on_delete);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    debug!("Terminal restored");

    Ok(res?)
}

/// Starts the history UI and returns the selected history entry.
///
/// # Errors
///
/// Returns an error if terminal setup/teardown fails or if input/rendering fails.
pub fn start(
    tracker: &mut Tracker,
    hide_instructions: bool,
    hide_info: bool,
) -> Result<Option<(EntryId, Entry)>> {
    let items = sorted_history_items(&tracker.history);
    let opts = PickerOpts {
        hide_instructions,
        hide_info,
    };

    let mut on_delete = |item: &HistoryItem| {
        let _ = tracker.history.delete(item.id);
    };

    let selected = pick(items, opts, Some(&mut on_delete))?;
    Ok(selected.map(|item| (item.id, item.entry)))
}

fn sorted_history_items(history: &History) -> Vec<HistoryItem> {
    let mut items: Vec<HistoryItem> = history
        .iter()
        .map(|(id, entry)| HistoryItem {
            id: *id,
            entry: entry.clone(),
        })
        .collect();

    items.sort_by_key(|item| -item.entry.last_opened.timestamp());
    items
}

fn run_app<T: Pickable>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: PickerState<'_, T>,
    on_delete: Option<&mut dyn FnMut(&T)>,
) -> io::Result<Option<T>> {
    app.table_state.select(Some(0));
    let mut on_delete = on_delete;

    loop {
        terminal.draw(|f| render(f, &mut app))?;

        let input = event::read()?;
        let action = handle_input(&input);

        if let Some(action) = action {
            match action {
                AppAction::Quit => return Ok(None),
                AppAction::SelectNext => {
                    app.select_next();
                    app.last_clicked_index = None;
                }
                AppAction::SelectPrevious => {
                    app.select_previous();
                    app.last_clicked_index = None;
                }
                AppAction::SelectFirst => {
                    app.select_first();
                    app.last_clicked_index = None;
                }
                AppAction::SelectLast => {
                    app.select_last();
                    app.last_clicked_index = None;
                }
                AppAction::OpenSelected => {
                    if let Some(selected) = app.get_selected_row() {
                        return Ok(Some(selected.item));
                    }
                }
                AppAction::DeleteSelectedEntry => {
                    if let Some(selected) = app.get_selected_row()
                        && let Some(callback) = on_delete.as_deref_mut()
                    {
                        callback(&selected.item);
                        let _ = app.delete(selected.original_index);
                    }
                    app.last_clicked_index = None;
                }
                AppAction::TableClick(row) => {
                    let table_area = terminal.get_frame().area();
                    if row >= table_area.y + 2 && row < table_area.y + table_area.height - 1 {
                        let clicked_index = usize::from(row - table_area.y - 2);
                        let visible_rows = app.table_data.as_rows_full().count();

                        if clicked_index < visible_rows {
                            if app.last_clicked_index == Some(clicked_index)
                                && app.table_state.selected() == Some(clicked_index)
                            {
                                if let Some(selected) = app.get_selected_row() {
                                    return Ok(Some(selected.item));
                                }
                            } else {
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
                        app.last_clicked_index = None;
                    }
                }
            }
        }
    }
}

fn handle_input(input: &Event) -> Option<AppAction> {
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

            return Some(AppAction::SearchInput((*key).into()));
        }
        Event::Mouse(MouseEvent { kind, row, .. }) => match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                return Some(AppAction::TableClick(*row));
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

    None
}

fn render<T: Pickable>(frame: &mut Frame, app: &mut PickerState<'_, T>) {
    let selected = app.get_selected_row();
    let status_lines = if app.opts.hide_info {
        Vec::new()
    } else {
        selected
            .as_ref()
            .map_or_else(Vec::new, |row| row.item.status_lines())
    };

    let mut constraints = vec![
        Constraint::Percentage(100),
        Constraint::Min(3),
        Constraint::Min(1),
    ];
    if !app.opts.hide_info {
        constraints.extend((0..status_lines.len()).map(|_| Constraint::Min(1)));
    }

    let area = Layout::default()
        .constraints(&constraints)
        .horizontal_margin(1)
        .split(frame.area());

    render_table(frame, app, area[0]);
    render_search_input(frame, app, area[1]);
    render_status_area(
        frame,
        &status_lines,
        &area[2..],
        app.opts.hide_instructions,
        app.opts.hide_info,
    );
}

fn render_search_input<T: Pickable>(frame: &mut Frame, app: &mut PickerState<'_, T>, area: Rect) {
    let style = Style::default().fg(Color::Blue);

    app.search.set_block(
        Block::default()
            .borders(Borders::all())
            .title("Search")
            .border_style(style),
    );

    frame.render_widget(&app.search, area);
}

fn render_table<T: Pickable>(frame: &mut Frame, app: &mut PickerState<'_, T>, area: Rect) {
    let (header_style, selected_style) = (
        Style::default().bg(Color::Blue),
        Style::default().bg(Color::DarkGray),
    );

    let header_cells = T::headers()
        .iter()
        .map(|header| Cell::from(*header).style(Style::default().fg(Color::White)));
    let header = Row::new(header_cells).style(header_style).height(1);

    let widths = T::column_constraints(&app.table_data.max_column_widths);

    let table = Table::new(app.table_data.to_rows(), widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(T::title()))
        .row_highlight_style(selected_style)
        .highlight_symbol("> ");
    frame.render_stateful_widget(table, area, &mut app.table_state);

    let total_items = app.table_data.as_rows_full().count();
    let viewport_height = usize::from(area.height - 2);

    if total_items >= viewport_height {
        let mut scrollbar_state = ScrollbarState::default()
            .content_length(total_items)
            .viewport_content_length(viewport_height)
            .position(app.table_state.selected().unwrap_or(0));

        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 2,
            width: 1,
            height: area.height - 3,
        };

        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓")),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }
}

fn render_status_area(
    frame: &mut Frame,
    status_lines: &[String],
    areas: &[Rect],
    hide_instructions: bool,
    hide_info: bool,
) {
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

    if !hide_info && areas.len() > 1 {
        for (index, line) in status_lines.iter().enumerate() {
            if let Some(area) = areas.get(index + 1) {
                let info = Span::styled(line.clone(), Style::default().fg(Color::DarkGray));
                let paragraph = Paragraph::new(info)
                    .block(Block::default().padding(Padding::new(2, 2, 0, 0)))
                    .alignment(Alignment::Left);
                frame.render_widget(paragraph, *area);
            }
        }
    }
}

/// Picker item wrapping a Docker devcontainer.
#[derive(Clone, Debug)]
pub struct ContainerItem(pub crate::container::Container);

impl Pickable for ContainerItem {
    fn title() -> &'static str {
        "Devcontainers"
    }
    fn headers() -> &'static [&'static str] {
        &["Container ID", "Status", "Image", "Project Path"]
    }
    fn cells(&self) -> Vec<String> {
        vec![
            self.0.short_id.clone(),
            self.0.status.clone(),
            self.0.image.clone(),
            self.0.local_folder.clone(),
        ]
    }
    fn search_fields(&self) -> Vec<String> {
        vec![
            self.0.short_id.clone(),
            self.0.status.clone(),
            self.0.image.clone(),
            self.0.local_folder.clone(),
            self.0.config_file.clone(),
        ]
    }
    fn status_lines(&self) -> Vec<String> {
        vec![format!("Config: {}", self.0.config_file)]
    }
    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let status_w = max_widths.get(1).copied().unwrap_or(6).clamp(6, 30);
        vec![
            Constraint::Min(13),
            Constraint::Min(u16::try_from(status_w).unwrap_or(6)),
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ]
    }
}

/// Launches a picker for Docker devcontainers.
pub fn pick_container(
    containers: Vec<crate::container::Container>,
    opts: PickerOpts,
    on_delete: Option<&mut dyn FnMut(&ContainerItem)>,
) -> Result<Option<crate::container::Container>> {
    let items: Vec<ContainerItem> = containers.into_iter().map(ContainerItem).collect();
    let selected = pick(items, opts, on_delete)?;
    Ok(selected.map(|item| item.0))
}

/// Picker item wrapping a stored devcontainer config.
#[derive(Clone, Debug)]
pub struct ConfigItem(pub crate::config_store::ConfigEntry);

impl Pickable for ConfigItem {
    fn title() -> &'static str {
        "Configs"
    }
    fn headers() -> &'static [&'static str] {
        &["Name", "Description", "Path"]
    }
    fn cells(&self) -> Vec<String> {
        vec![
            self.0.name.clone(),
            self.0.description.as_deref().unwrap_or("").to_string(),
            self.0.root.display().to_string(),
        ]
    }
    fn search_fields(&self) -> Vec<String> {
        vec![
            self.0.name.clone(),
            self.0.description.clone().unwrap_or_default(),
            self.0.root.to_string_lossy().to_string(),
        ]
    }
    fn status_lines(&self) -> Vec<String> {
        vec![]
    }
    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let name_w = max_widths.first().copied().unwrap_or(4).clamp(4, 30);
        let desc_w = max_widths.get(1).copied().unwrap_or(4).clamp(4, 40);
        vec![
            Constraint::Min(u16::try_from(name_w).unwrap_or(4)),
            Constraint::Min(u16::try_from(desc_w).unwrap_or(4)),
            Constraint::Percentage(70),
        ]
    }
}

/// Launches a picker for stored devcontainer configs.
pub fn pick_config(
    configs: Vec<crate::config_store::ConfigEntry>,
    opts: PickerOpts,
    on_delete: Option<&mut dyn FnMut(&ConfigItem)>,
) -> Result<Option<crate::config_store::ConfigEntry>> {
    let items: Vec<ConfigItem> = configs.into_iter().map(ConfigItem).collect();
    let selected = pick(items, opts, on_delete)?;
    Ok(selected.map(|item| item.0))
}

/// Picker item for selecting among multiple devcontainer configs in a project.
#[derive(Clone, Debug)]
pub struct DevContainerItem(pub crate::workspace::DevContainer);

impl Pickable for DevContainerItem {
    fn title() -> &'static str {
        "Select Dev Container"
    }
    fn headers() -> &'static [&'static str] {
        &["Name", "Config Path"]
    }
    fn cells(&self) -> Vec<String> {
        vec![
            self.0.name.as_deref().unwrap_or("(unnamed)").to_string(),
            self.0.config_path.display().to_string(),
        ]
    }
    fn search_fields(&self) -> Vec<String> {
        vec![
            self.0.name.clone().unwrap_or_default(),
            self.0.config_path.to_string_lossy().to_string(),
        ]
    }
    fn status_lines(&self) -> Vec<String> {
        vec![format!("Workspace: {}", self.0.workspace_path_in_container)]
    }
    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let name_w = max_widths.first().copied().unwrap_or(9).clamp(9, 40);
        vec![
            Constraint::Min(u16::try_from(name_w).unwrap_or(9)),
            Constraint::Percentage(80),
        ]
    }
}

/// Launches a picker for devcontainer selection.
pub fn pick_devcontainer(
    dev_containers: Vec<crate::workspace::DevContainer>,
) -> Result<Option<crate::workspace::DevContainer>> {
    let items: Vec<DevContainerItem> = dev_containers.into_iter().map(DevContainerItem).collect();
    let opts = PickerOpts {
        hide_instructions: false,
        hide_info: false,
    };
    let selected = pick(items, opts, None)?;
    Ok(selected.map(|item| item.0))
}

fn add_num_opt(o1: Option<u32>, o2: Option<u32>) -> Option<u32> {
    match (o1, o2) {
        (Some(n1), Some(n2)) => Some(n1 + n2),
        (Some(n), None) | (None, Some(n)) => Some(n),
        _ => None,
    }
}
