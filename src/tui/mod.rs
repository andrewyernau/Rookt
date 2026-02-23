pub mod app;
pub mod config_screen;
pub mod dashboard;

use crate::events::{ChannelSink, PipelineControl, UiEvent};
use crate::pipeline;
use app::{App, RunState, Screen};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Run the full TUI application.
pub fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let result = main_loop(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        // Render
        terminal.draw(|f| match app.screen {
            Screen::Config => config_screen::render(f, app),
            Screen::Dashboard => dashboard::render(f, app),
        })?;

        // Poll for pipeline events
        let events: Vec<_> = app.event_rx.as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            app.handle_event(event);
        }

        // Poll for input events (50ms timeout for ~20fps)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events, not release/repeat (avoids double-input on Windows)
                if key.kind != event::KeyEventKind::Press {
                    continue;
                }

                // Ctrl+C always quits
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    if let Some(control) = &app.control {
                        control.cancel();
                    }
                    break;
                }

                match app.screen {
                    Screen::Config => handle_config_key(app, key),
                    Screen::Dashboard => handle_dashboard_key(app, key),
                }
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

// ── Config screen key handling ──────────────────────────────────────────────

fn handle_config_key(app: &mut App, key: event::KeyEvent) {
    if app.editing {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                app.editing = false;
            }
            KeyCode::Char(c) => {
                let cursor = app.edit_cursor.min(app.fields[app.selected].value.len());
                app.fields[app.selected].value.insert(cursor, c);
                app.edit_cursor = cursor + 1;
            }
            KeyCode::Backspace => {
                if app.edit_cursor > 0 {
                    let cursor = app.edit_cursor.min(app.fields[app.selected].value.len());
                    app.fields[app.selected].value.remove(cursor - 1);
                    app.edit_cursor = cursor - 1;
                }
            }
            KeyCode::Delete => {
                let cursor = app.edit_cursor.min(app.fields[app.selected].value.len());
                if cursor < app.fields[app.selected].value.len() {
                    app.fields[app.selected].value.remove(cursor);
                }
            }
            KeyCode::Left => {
                if app.edit_cursor > 0 {
                    app.edit_cursor -= 1;
                }
            }
            KeyCode::Right => {
                if app.edit_cursor < app.fields[app.selected].value.len() {
                    app.edit_cursor += 1;
                }
            }
            KeyCode::Home => app.edit_cursor = 0,
            KeyCode::End => app.edit_cursor = app.fields[app.selected].value.len(),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Up => {
            if app.selected > 0 {
                app.selected -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected < app.total_items() - 1 {
                app.selected += 1;
            }
        }
        KeyCode::Tab => {
            app.selected = (app.selected + 1) % app.total_items();
        }
        KeyCode::BackTab => {
            if app.selected == 0 {
                app.selected = app.total_items() - 1;
            } else {
                app.selected -= 1;
            }
        }
        KeyCode::Enter => {
            if app.is_on_start_button() {
                try_start_pipeline(app);
            } else {
                app.editing = true;
                app.edit_cursor = app.fields[app.selected].value.len();
            }
        }
        KeyCode::F(5) => try_start_pipeline(app),
        _ => {}
    }
}

fn try_start_pipeline(app: &mut App) {
    match app.build_config() {
        Ok(config) => {
            app.validation_error = None;
            start_pipeline(app, config);
        }
        Err(err) => {
            app.validation_error = Some(err);
        }
    }
}

fn start_pipeline(app: &mut App, config: crate::config::Config) {
    let (tx, rx) = mpsc::channel::<UiEvent>();
    let control = Arc::new(PipelineControl::new());
    let sink = ChannelSink::new(tx.clone(), control.clone());

    app.event_rx = Some(rx);
    app.control = Some(control);
    app.screen = Screen::Dashboard;
    app.run_state = RunState::Running;

    std::thread::spawn(move || {
        let result = pipeline::run_with_sink(&config, sink);
        match result {
            Ok(()) => { let _ = tx.send(UiEvent::Finished); }
            Err(e) => { let _ = tx.send(UiEvent::Error(e.to_string())); }
        }
    });
}

// ── Dashboard key handling ──────────────────────────────────────────────────

fn handle_dashboard_key(app: &mut App, key: event::KeyEvent) {
    match key.code {
        KeyCode::Char('q') => {
            if let Some(control) = &app.control {
                control.cancel();
            }
            app.should_quit = true;
        }
        KeyCode::Char('p') => {
            if app.run_state == RunState::Running {
                if let Some(control) = &app.control {
                    control.pause();
                }
                app.run_state = RunState::Paused;
            }
        }
        KeyCode::Char('r') => {
            if app.run_state == RunState::Paused {
                if let Some(control) = &app.control {
                    control.resume();
                }
                app.run_state = RunState::Running;
            }
        }
        KeyCode::Up => {
            if app.log_scroll > 0 {
                app.log_scroll -= 1;
            }
        }
        KeyCode::Down => {
            if app.log_scroll < app.logs.len().saturating_sub(1) {
                app.log_scroll += 1;
            }
        }
        KeyCode::PageUp => {
            app.log_scroll = app.log_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.log_scroll = (app.log_scroll + 10).min(app.logs.len().saturating_sub(1));
        }
        _ => {}
    }
}
