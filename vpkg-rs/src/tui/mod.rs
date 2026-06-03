pub mod app;
pub mod ui;

use crate::package::{Package, Source};
use crate::resolver::Resolver;
use anyhow::Result;
use app::{App, Mode};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub async fn run(
    resolver: Arc<Resolver>,
    initial_query: Option<String>,
    filter: Option<Source>,
) -> Result<Option<Vec<Package>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.filter = filter;
    if let Some(q) = initial_query {
        app.search_input = q;
        app.last_input = Instant::now() - Duration::from_secs(1); // trigger search immediately
    }

    let (results_tx, mut results_rx) = mpsc::unbounded_channel::<Vec<Package>>();
    let result = event_loop(&mut terminal, &mut app, &resolver, results_tx, &mut results_rx).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    resolver: &Arc<Resolver>,
    results_tx: mpsc::UnboundedSender<Vec<Package>>,
    results_rx: &mut mpsc::UnboundedReceiver<Vec<Package>>,
) -> Result<Option<Vec<Package>>> {
    let tick = Duration::from_millis(16);
    let debounce = Duration::from_millis(350);
    let mut last_tick = Instant::now();
    let mut last_searched = String::new();

    loop {
        terminal.draw(|f| ui::render(f, app))?;

        // Poll for key events with a short timeout so the loop stays snappy
        if event::poll(tick.saturating_sub(last_tick.elapsed()))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key.code, key.modifiers);
            }
        }

        // Drain any search results that came back from background tasks
        while let Ok(results) = results_rx.try_recv() {
            app.on_results(results);
        }

        // Debounced search: fire if input changed and the user paused typing
        if !app.search_input.is_empty()
            && app.search_input != last_searched
            && app.last_input.elapsed() >= debounce
        {
            last_searched = app.search_input.clone();
            app.is_loading = true;
            app.status = String::from(" Searching…");

            let q = last_searched.clone();
            let res = Arc::clone(resolver);
            let tx = results_tx.clone();
            tokio::spawn(async move {
                let results = res.search_all(&q).await.unwrap_or_default();
                let _ = tx.send(results);
            });
        }

        // Clear results when search box is emptied
        if app.search_input.is_empty() && !last_searched.is_empty() {
            last_searched.clear();
            app.search_results.clear();
        }

        if last_tick.elapsed() >= tick {
            app.tick();
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(None);
        }
        if let Some(pkgs) = app.packages_to_install.take() {
            return Ok(Some(pkgs));
        }

        tokio::time::sleep(Duration::from_millis(0)).await;
    }
}

fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // Help overlay captures all keys
    if app.mode == Mode::Help {
        match code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                app.mode = Mode::Browse;
            }
            _ => {}
        }
        return;
    }

    // Source picker overlay captures all keys
    if let Mode::SourcePick(sources) = app.mode.clone() {
        match code {
            KeyCode::Esc => app.mode = Mode::Browse,
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let idx = c.to_digit(10).unwrap_or(0) as usize;
                if idx > 0 && idx <= sources.len() {
                    let source = sources[idx - 1].clone();
                    let targets: Vec<Package> = app
                        .search_results
                        .iter()
                        .filter(|p| p.source == source)
                        .enumerate()
                        .filter(|(i, _)| app.marked.is_empty() || app.marked.contains(i))
                        .map(|(_, p)| p.clone())
                        .collect();
                    if !targets.is_empty() {
                        app.packages_to_install = Some(targets);
                    }
                    app.mode = Mode::Browse;
                }
            }
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Esc => {
            if app.mode == Mode::Browse {
                app.mode = Mode::Search;
            } else if !app.search_input.is_empty() {
                app.clear_search();
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Char('q') if app.mode == Mode::Browse && modifiers.is_empty() => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.mode = Mode::Help;
        }
        KeyCode::Tab => {
            app.cycle_filter();
        }
        KeyCode::Up => {
            app.mode = Mode::Browse;
            app.move_up();
        }
        KeyCode::Down => {
            app.mode = Mode::Browse;
            app.move_down();
        }
        KeyCode::Char(' ') if app.mode == Mode::Browse => {
            app.toggle_mark();
        }
        KeyCode::Char('i') | KeyCode::Enter => {
            app.resolve_install();
        }
        KeyCode::Backspace => {
            app.search_input.pop();
            app.last_input = Instant::now();
            if app.search_input.is_empty() {
                app.search_results.clear();
                app.marked.clear();
                app.selected = 0;
                app.status =
                    String::from(" Type to search · Tab: filter · ? help · q quit");
            }
            app.mode = Mode::Search;
        }
        KeyCode::Char(c) if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            app.search_input.push(c);
            app.last_input = Instant::now();
            app.mode = Mode::Search;
        }
        _ => {}
    }
}
