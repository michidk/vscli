use color_eyre::eyre::{Result, eyre};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame, Terminal,
};
use std::{borrow::Cow, io};

use crate::history::{Tracker, Entry};

struct UI<'a, 'b> {
    state: TableState,
    tracker: &'a mut Tracker<'b>,
}

impl<'a, 'b> UI<'a, 'b> {
    fn new(tracker: &'a mut Tracker<'b>) -> UI<'a, 'b> {
        UI {
            state: TableState::default(),
            tracker,
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.tracker.history.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tracker.history.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

pub(crate) fn start<'a, 'b>(tracker: &'a mut Tracker<'b>) -> Result<Option<Entry>> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;


    // create app and run it
    let res = run_app(&mut terminal, UI::<'a, 'b>::new(tracker));

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    match res {
        Ok(None) => {},
        Ok(Some(index)) => {
            return Ok(tracker.history.iter().nth(index).map(Clone::clone))
        }
        Err(message) => Err(eyre!("Error: {:?}", message))?,
    }

    Ok(None)
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: UI) -> io::Result<Option<usize>> {
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
                _ => {}
            }
        }
    }
}

fn render<B: Backend>(f: &mut Frame<B>, app: &mut UI) {
    let rects = Layout::default()
        .constraints([Constraint::Percentage(100)].as_ref())
        .margin(1)
        .split(f.size());

    let selected_style = Style::default().bg(Color::Gray);
    let normal_style = Style::default().bg(Color::Blue);
    let header_cells = ["Name", "Path", "Last Opened"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::White)));
    let header = Row::new(header_cells).style(normal_style).height(1);
    let rows = app.tracker.history.iter().map(|item| {
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
    let t = Table::new(rows)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("VSCLI - Recent Workspaces"),
        )
        .highlight_style(selected_style)
        .highlight_symbol("> ")
        .widths(&[
            Constraint::Percentage(20),
            Constraint::Percentage(50),
            Constraint::Percentage(30),
        ]);
    f.render_stateful_widget(t, rects[0], &mut app.state);
}
