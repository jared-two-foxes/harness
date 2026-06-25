mod app;
mod extensions;
mod linear;
mod markdown;
mod project;
mod ui;

use std::{io::stdout, time::Duration};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, oneshot};

use app::{App, Mode};
use extensions::ExtensionEvent;
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

    match extensions::load() {
        Ok(exts) => app.extensions = exts,
        Err(e) => eprintln!("warning: failed to load extensions config: {e:?}"),
    }

    match project::load() {
        Ok(projects) => match std::env::current_dir() {
            Ok(cwd) => {
                if let Some(active) = project::find_active(&projects, &cwd) {
                    app.set_active_project(active.clone());
                }
            }
            Err(e) => eprintln!("warning: failed to determine current directory: {e:?}"),
        },
        Err(e) => eprintln!("warning: failed to load projects config: {e:?}"),
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

/// Triggers the extension bound to `key`, if any. If one is already running,
/// just reopens its (still-updating) output view instead of starting a
/// second concurrent run — important for scripts like check-ticket.py /
/// resolve-ticket.py that mutate shared state files in the target repo.
/// Otherwise spawns the command in the background and streams its output
/// back through `tx` without blocking the UI loop.
fn launch_extension(app: &mut App, key: char, tx: mpsc::UnboundedSender<ExtensionEvent>) {
    let Some(extension) = app.find_extension(key) else {
        return;
    };
    if app.extension_running() {
        app.show_extension_output();
        return;
    }
    let Some(issue) = app.selected_issue().cloned() else {
        return;
    };
    let project_root = app.project_root();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    app.start_extension(extension.name.clone(), cancel_tx);
    tokio::spawn(extensions::run(extension, issue, project_root, tx, cancel_rx));
}

/// Fetches the selected issue's team's available statuses and opens the
/// picker. A no-op if nothing is selected.
async fn open_status_picker(app: &mut App, client: &Client) {
    let Some(issue) = app.selected_issue().cloned() else {
        return;
    };
    match client.fetch_workflow_states(&issue.team.name).await {
        Ok(states) => app.open_status_picker(states),
        Err(e) => app.set_error(format!("{e:?}")),
    }
}

/// Resolves the active project's team/project to ids and creates the issue
/// against Linear, dropping it into the list on success. Only callable while
/// `app.active_project` is set, since that's where the team/project mapping
/// comes from.
async fn create_issue(app: &mut App, client: &Client, title: String) {
    let Some(project) = app.active_project.clone() else {
        return;
    };
    match client
        .resolve_team_project_ids(&project.team, Some(&project.project))
        .await
    {
        Ok((team_id, project_id)) => {
            match client
                .create_issue(&team_id, project_id.as_deref(), &title)
                .await
            {
                Ok(issue) => app.add_issue(issue),
                Err(e) => app.set_error(format!("{e:?}")),
            }
        }
        Err(e) => app.set_error(format!("{e:?}")),
    }
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    client: &Client,
) -> Result<()> {
    let (ext_tx, mut ext_rx) = mpsc::unbounded_channel::<ExtensionEvent>();

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Drained every tick regardless of which mode/screen is active, so
        // a script's output is never lost just because the user navigated
        // away from its output view while it was still running.
        while let Ok(event) = ext_rx.try_recv() {
            match event {
                ExtensionEvent::Line { name, stderr, text } => {
                    app.push_extension_line(&name, stderr, text)
                }
                ExtensionEvent::Done { name, success } => app.finish_extension_run(&name, success),
            }
        }

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match &app.mode {
                        Mode::Normal => match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                            KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                            KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                            KeyCode::Char('f') => app.open_filter_menu(),
                            KeyCode::Char('o') => app.toggle_sort(),
                            KeyCode::Enter | KeyCode::Char('l') => app.open_detail(),
                            KeyCode::Char('r') => match client.fetch_my_issues().await {
                                Ok(issues) => app.set_issues(issues),
                                Err(e) => app.set_error(format!("{e:?}")),
                            },
                            KeyCode::Char('s') => open_status_picker(app, client).await,
                            KeyCode::Char('n') => {
                                if app.active_project.is_some() {
                                    app.open_new_issue();
                                }
                            }
                            KeyCode::Char(c) => launch_extension(app, c, ext_tx.clone()),
                            _ => {}
                        },
                        Mode::Detail => match key.code {
                            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('h') => {
                                app.close_detail()
                            }
                            KeyCode::Char('j') | KeyCode::Down => app.scroll_detail(1),
                            KeyCode::Char('k') | KeyCode::Up => app.scroll_detail(-1),
                            KeyCode::PageDown | KeyCode::Char('d') => app.scroll_detail(10),
                            KeyCode::PageUp | KeyCode::Char('u') => app.scroll_detail(-10),
                            KeyCode::Char('g') => app.scroll_detail(i32::MIN),
                            KeyCode::Char('G') => app.scroll_detail(i32::MAX),
                            KeyCode::Char('s') => open_status_picker(app, client).await,
                            KeyCode::Char(c) => launch_extension(app, c, ext_tx.clone()),
                            _ => {}
                        },
                        Mode::StatusPicker { .. } => match key.code {
                            KeyCode::Esc => app.status_picker_cancel(),
                            KeyCode::Char('j') | KeyCode::Down => app.status_picker_move(1),
                            KeyCode::Char('k') | KeyCode::Up => app.status_picker_move(-1),
                            KeyCode::Enter => {
                                if let Some((issue_id, new_state)) = app.status_picker_confirm() {
                                    match client.update_issue_state(&issue_id, &new_state.id).await {
                                        Ok(()) => app.apply_state_change(&issue_id, new_state),
                                        Err(e) => app.set_error(format!("{e:?}")),
                                    }
                                }
                            }
                            _ => {}
                        },
                        Mode::NewIssueTitle { .. } => match key.code {
                            KeyCode::Esc => app.new_issue_cancel(),
                            KeyCode::Backspace => app.new_issue_backspace(),
                            KeyCode::Enter => {
                                if let Some(title) = app.new_issue_confirm() {
                                    create_issue(app, client, title).await;
                                }
                            }
                            KeyCode::Char(c) => app.new_issue_input(c),
                            _ => {}
                        },
                        Mode::ExtensionOutput { .. } => match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => app.close_extension_output(),
                            KeyCode::Char('K') => app.cancel_running_extension(),
                            KeyCode::Char('j') | KeyCode::Down => app.scroll_extension_output(1),
                            KeyCode::Char('k') | KeyCode::Up => app.scroll_extension_output(-1),
                            KeyCode::PageDown | KeyCode::Char('d') => {
                                app.scroll_extension_output(10)
                            }
                            KeyCode::PageUp | KeyCode::Char('u') => {
                                app.scroll_extension_output(-10)
                            }
                            KeyCode::Char('g') => app.scroll_extension_output(i32::MIN),
                            KeyCode::Char('G') => app.scroll_extension_output(i32::MAX),
                            _ => {}
                        },
                        Mode::FilterMenu { .. } => match key.code {
                            KeyCode::Esc => app.filter_menu_cancel(),
                            KeyCode::Enter => app.filter_menu_select(),
                            KeyCode::Char('j') | KeyCode::Down => app.filter_menu_move(1),
                            KeyCode::Char('k') | KeyCode::Up => app.filter_menu_move(-1),
                            KeyCode::Char('c') => app.clear_filters(),
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
