use color_eyre::eyre::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame, Terminal,
};

use crate::history::{Entry, Tracker};

struct UI<'a> {
    state: TableState,
    tracker: &'a Tracker<'a>,
    history: Vec<Entry>,
    // items: Vec<Item<'a>>,
}

// struct Item<'a> {
//     entry: &'a Entry,
//     formatted_time: String,
// }

impl<'a> UI<'a> {
    fn new(tracker: &'a Tracker) -> UI<'a> {
        let history = tracker.get_sorted_history_vec();
        // let mut items: Vec<Item> = vec![];
        // for entry in history {
        //     items.push(Item {
        //         entry: &entry,
        //         formatted_time: entry
        //             .last_opened
        //             .clone()
        //             .format("%Y-%m-%d %H:%M:%S")
        //             .to_string(),
        //     });
        // }
        // let items = history.iter().map(|entry| {
        //     Item {
        //         entry,
        //         formatted_time: entry.last_opened.clone().format("%Y-%m-%d %H:%M:%S").to_string(),
        //     }
        // }).collect();

        UI {
            state: TableState::default(),
            tracker,
            history,
            // items,
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.history.len() - 1 {
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
                    self.history.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

pub(crate) fn start(tracker: &Tracker) -> Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = UI::new(tracker);
    let res = run_app(&mut terminal, app);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: UI) -> io::Result<()> {
    loop {
        terminal.draw(|f| render(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => return Ok(()),
                KeyCode::Down => app.next(),
                KeyCode::Up => app.previous(),
                _ => {}
            }
        }
    }
}

fn render<B: Backend>(f: &mut Frame<B>, app: &mut UI) {
    let rects = Layout::default()
        .constraints([Constraint::Percentage(100)].as_ref())
        .margin(5)
        .split(f.size());

    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let normal_style = Style::default().bg(Color::Blue);
    let header_cells = ["Name", "Path", "Last Opened"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Red)));
    let header = Row::new(header_cells)
        .style(normal_style)
        .height(1)
        .bottom_margin(1);
    let rows = app.history.iter().map(|item| {
        let cells: Vec<&str> = vec![
            &item.name,
            item.path
                .to_str()
                .expect("Could not convert path to string."),
            "test", // &item.formatted_time,
        ];

        Row::new(cells).height(1).bottom_margin(1)
    });
    let t = Table::new(rows)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recent workspaces"),
        )
        .highlight_style(selected_style)
        .highlight_symbol("> ")
        .widths(&[
            Constraint::Percentage(50),
            Constraint::Length(30),
            Constraint::Min(10),
        ]);
    f.render_stateful_widget(t, rects[0], &mut app.state);
}
