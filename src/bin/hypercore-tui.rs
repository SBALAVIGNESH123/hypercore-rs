use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use reqwest::blocking::Client;
use std::{
    error::Error,
    io,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = Client::new();

    let res = run_app(&mut terminal, &client);

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

fn run_app<B: Backend>(terminal: &mut Terminal<B>, client: &Client) -> io::Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(500);

    let mut metrics_text = String::from("Loading metrics...");

    loop {
        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Min(2),
                        Constraint::Length(3),
                    ]
                    .as_ref(),
                )
                .split(size);

            let header = Paragraph::new("HYPERCORE Live Operator Dashboard")
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .block(Block::default().borders(Borders::ALL).title("Status"));
            f.render_widget(header, chunks[0]);

            let content = Paragraph::new(metrics_text.as_str()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Metrics (Prometheus)"),
            );
            f.render_widget(content, chunks[1]);

            let footer = Paragraph::new("Press 'q' to quit")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        })?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if let KeyCode::Char('q') = key.code {
                    return Ok(());
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            // Poll metrics
            if let Ok(res) = client.get("http://127.0.0.1:8080/metrics").send() {
                if let Ok(text) = res.text() {
                    metrics_text = text;
                }
            } else {
                metrics_text = String::from(
                    "Failed to connect to HYPERCORE server at http://127.0.0.1:8080/metrics",
                );
            }
            last_tick = Instant::now();
        }
    }
}
