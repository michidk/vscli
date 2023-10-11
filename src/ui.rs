use color_eyre::eyre::{eyre, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    prelude::Alignment,
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Padding, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use std::{borrow::Cow, io};

use crate::history::{Entry, Tracker};

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
    // setup terminal
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

    match res {
        Ok(None) => {}
        Ok(Some(index)) => return Ok(tracker.history.iter().nth(index).map(Clone::clone)),
        Err(message) => Err(eyre!("Error: {:?}", message))?,
    }

    Ok(None)
}

/// UI main loop
fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: UI) -> io::Result<Option<usize>> {
    app.state.select(Some(0)); // Select the most recent element by default
    loop {
        terminal.draw(|f| render(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => return Ok(None),
                KeyCode::Down => app.next(),
                KeyCode::Up => app.previous(),
                KeyCode::Enter | KeyCode::Char('o') => {
                    if let Some(selected) = app.state.selected() {
                        return Ok(Some(selected));
                    }
                }
                KeyCode::Delete | KeyCode::Char('x') => {
                    if let Some(selected) = app.state.selected() {
                        let entry = app.tracker.history.iter().nth(selected).unwrap().clone();
                        app.tracker.history.remove(&entry);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Renders the UI
fn render<B: Backend>(frame: &mut Frame<B>, app: &mut UI) {
    // Setup crossterm UI layout & style
    let area = Layout::default()
        .constraints([Constraint::Percentage(100), Constraint::Min(2)].as_ref())
        .margin(1)
        .split(frame.size());

    let selected_style = Style::default().bg(Color::DarkGray);
    let normal_style = Style::default().bg(Color::Blue);
    let header_cells = ["Name", "Path", "Last Opened"]
        .iter()
        .map(|header| Cell::from(*header).style(Style::default().fg(Color::White)));
    let header = Row::new(header_cells).style(normal_style).height(1);
    let rows = app.tracker.history.iter().rev().map(|item| {
        let cells: Vec<Cow<'_, str>> = vec![
            Cow::Borrowed(&item.name),
            item.path.to_string_lossy(),
            Cow::Owned(
                item.last_opened
                    .clone()
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            ),
        ];

        Row::new(cells).height(1)
    });

    // Limit the length of workspace names displayed
    let longest_name: u16 = u16::min(
        app.tracker
            .history
            .iter()
            .map(|entry| entry.name.clone())
            .max_by_key(String::len)
            .unwrap_or("0123467890123456789".to_string())
            .len()
            .try_into()
            .unwrap_or(20),
        60,
    );
    let widths = [
        Constraint::Min(longest_name + 1),
        Constraint::Percentage(100),
        Constraint::Min(20),
    ];

    // Setup the table
    let table = Table::new(rows)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("VSCLI - Recent Workspaces"),
        )
        .highlight_style(selected_style)
        .highlight_symbol("> ")
        .widths(&widths);
    frame.render_stateful_widget(table, area[0], &mut app.state);

    let selected = app
        .tracker
        .history
        .iter()
        .nth(app.state.selected().unwrap_or(0));

    // Gather additional information for the status area
    let strategy = map_or_default(selected, "-", |item| item.behavior.strategy.to_string());
    let insiders = map_or_default(selected, "-", |entry| entry.behavior.insiders.to_string());
    let args_count = map_or_default(selected, "0", |entry| entry.behavior.args.len().to_string());
    let args = selected.map_or(String::from("-"), |entry| {
        let converted_str: Vec<&str> = entry
            .behavior
            .args
            .iter()
            .map(|os_str| {
                os_str
                    .to_str()
                    .expect("Failed to convert `OsStr` into `&str`")
            })
            .collect();
        converted_str.join(" ")
    });

    // Render status area
    let additional_info = Span::styled(
        format!("Strategy: {strategy}, Insiders: {insiders}, Args ({args_count}): {args}"),
        Style::default().fg(Color::DarkGray),
    );

    let status_area = Layout::default()
        .direction(ratatui::prelude::Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(area[1]);

    let status_block = Block::default().padding(Padding::new(5, 5, 0, 0));
    let additional_info_par = Paragraph::new(additional_info)
        .block(status_block.clone())
        .alignment(Alignment::Right);
    frame.render_widget(additional_info_par, status_area[1]);

    // Instructions
    let instruction = Span::styled(
        "Press x to remove the selected item.",
        Style::default().fg(Color::DarkGray),
    );
    let instructions_par = Paragraph::new(instruction)
        .block(status_block)
        .alignment(Alignment::Left);

    // Render
    frame.render_widget(instructions_par, status_area[0]);
}

/// Maps an option to a string, using a default value if the option is `None`
fn map_or_default<T, F: Fn(T) -> String>(option: Option<T>, default: &str, f: F) -> String {
    option.map_or(default.to_string(), f)
}
