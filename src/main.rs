mod app;
mod linear;
mod markdown;
mod ui;

use std::{io::stdout, time::Duration};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, Mode};
use linear::Client;

fn load_api_key() -> Result<String> {
    if let Ok(key) = std::env::var("LINEAR_API_KEY") {
        return Ok(key.trim().to_string());
    }

    let home = directories::BaseDirs::new().context("could not determine home directory")?;
    let path = home.home_dir().join(".secrets").join("linear-key");
    let key = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read Linear API key from {}", path.display()))?;
    Ok(key.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = load_api_key()?;
    let client = Client::new(api_key);

    let mut app = App::new();
    match client.fetch_my_issues().await {
        Ok(issues) => app.set_issues(issues),
        Err(e) => app.set_error(format!("{e:?}")),
    }

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, &mut app, &client).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    client: &Client,
) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match &app.mode {
                        Mode::Normal => match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                            KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                            KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                            KeyCode::Char('t') => app.open_team_filter(),
                            KeyCode::Char('p') => app.open_project_filter(),
                            KeyCode::Char('s') => app.open_status_filter(),
                            KeyCode::Char('c') => app.clear_filters(),
                            KeyCode::Char('r') => match client.fetch_my_issues().await {
                                Ok(issues) => app.set_issues(issues),
                                Err(e) => app.set_error(format!("{e:?}")),
                            },
                            _ => {}
                        },
                        Mode::Filter { .. } => match key.code {
                            KeyCode::Esc => app.filter_cancel(),
                            KeyCode::Enter => app.filter_confirm(),
                            KeyCode::Char(' ') => app.filter_toggle(),
                            KeyCode::Char('j') | KeyCode::Down => app.filter_move(1),
                            KeyCode::Char('k') | KeyCode::Up => app.filter_move(-1),
                            _ => {}
                        },
                    }
                }
            }
        }
    }
    Ok(())
}
