use crate::tui::app::{fmt_bytes, fmt_count, App, Phase, RunState};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(5),  // Progress
            Constraint::Length(8),  // Stats
            Constraint::Min(6),    // Logs
            Constraint::Length(3),  // Controls
        ])
        .split(area);

    render_header(f, chunks[0], app);
    render_progress(f, chunks[1], app);
    render_stats(f, chunks[2], app);
    render_logs(f, chunks[3], app);
    render_controls(f, chunks[4], app);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let state_span = match &app.run_state {
        RunState::Running => Span::styled(" RUNNING ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        RunState::Paused => Span::styled(" PAUSED ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        RunState::Finished => Span::styled(" FINISHED ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        RunState::Cancelled => Span::styled(" CANCELLED ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        RunState::Error(_) => Span::styled(" ERROR ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        RunState::Idle => Span::styled(" IDLE ", Style::default().fg(Color::DarkGray)),
    };

    let dataset_info = if app.total_datasets > 0 {
        format!(
            " [{}/{}] {}",
            app.current_dataset + 1,
            app.total_datasets,
            app.dataset_name
        )
    } else {
        String::new()
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" PGN Player Extractor ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        state_span,
        Span::styled(dataset_info, Style::default().fg(Color::White)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    f.render_widget(header, area);
}

fn render_progress(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Progress ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Overall
            Constraint::Length(1), // File / Download
            Constraint::Length(1), // Phase
        ])
        .split(inner);

    // Overall dataset progress
    let overall_pct = if app.total_datasets > 0 {
        (app.current_dataset as f64 / app.total_datasets as f64).min(1.0)
    } else {
        0.0
    };
    let overall_gauge = Gauge::default()
        .label(format!(
            "Overall: {}/{} datasets",
            app.current_dataset, app.total_datasets
        ))
        .ratio(overall_pct)
        .gauge_style(Style::default().fg(Color::Cyan));
    f.render_widget(overall_gauge, rows[0]);

    // File / Download progress
    let (file_pct, file_label) = match app.phase {
        Phase::Downloading => {
            let pct = if app.dl_total > 0 {
                app.dl_read as f64 / app.dl_total as f64
            } else {
                0.0
            };
            (pct, format!(
                "Download: {} / {}",
                fmt_bytes(app.dl_read),
                fmt_bytes(app.dl_total)
            ))
        }
        Phase::Pass1 | Phase::Pass2 => {
            let pct = if app.file_total > 0 {
                app.file_read as f64 / app.file_total as f64
            } else {
                0.0
            };
            (pct, format!(
                "File: {} / {} ({:.1}%)",
                fmt_bytes(app.file_read),
                fmt_bytes(app.file_total),
                pct * 100.0
            ))
        }
        _ => (0.0, "Idle".into()),
    };
    let file_gauge = Gauge::default()
        .label(file_label)
        .ratio(file_pct.min(1.0))
        .gauge_style(Style::default().fg(Color::Green));
    f.render_widget(file_gauge, rows[1]);

    // Current phase
    let phase_text = match app.phase {
        Phase::Downloading => "Phase: Downloading dataset...".to_string(),
        Phase::Pass1 => format!(
            "Phase: Pass 1 — Counting ({} scanned, {} valid, {} players)",
            fmt_count(app.p1_scanned), fmt_count(app.p1_valid), fmt_count(app.p1_players)
        ),
        Phase::Pass2 => format!(
            "Phase: Pass 2 — Extracting ({} entries)",
            fmt_count(app.p2_extracted)
        ),
        Phase::Pruning => "Phase: Final pruning...".to_string(),
        Phase::Done => "Phase: Complete".to_string(),
    };
    let phase = Paragraph::new(Line::from(Span::styled(
        phase_text,
        Style::default().fg(Color::White),
    )));
    f.render_widget(phase, rows[2]);
}

fn render_stats(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Stats ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    // Current dataset stats
    let current_stats = vec![
        Line::from(Span::styled(" Current Dataset", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(format!("  Games scanned:  {}", fmt_count(app.p1_scanned))),
        Line::from(format!("  Valid games:    {}", fmt_count(app.p1_valid))),
        Line::from(format!("  Players found:  {}", fmt_count(app.p1_players))),
        Line::from(format!("  Extracted:      {}", fmt_count(app.p2_extracted))),
    ];
    f.render_widget(Paragraph::new(current_stats), cols[0]);

    // Cumulative stats
    let total_stats = vec![
        Line::from(Span::styled(" Cumulative Totals", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(format!("  Qualifying players: {}", fmt_count(app.cum_qualifying))),
        Line::from(format!("  Games saved:        {}", fmt_count(app.cum_games_saved))),
        Line::from(format!("  Final players:      {}", fmt_count(app.final_players))),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(total_stats), cols[1]);
}

fn render_logs(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(" Logs ({}) ", app.logs.len()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_height = inner.height as usize;
    let total = app.logs.len();
    let start = app.log_scroll.min(total.saturating_sub(visible_height));

    let log_lines: Vec<Line> = app.logs
        .iter()
        .skip(start)
        .take(visible_height)
        .map(|msg| {
            let style = if msg.contains("ERROR") {
                Style::default().fg(Color::Red)
            } else if msg.contains("done") || msg.contains("complete") || msg.contains("finished") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(Span::styled(format!("  {}", msg), style))
        })
        .collect();

    f.render_widget(Paragraph::new(log_lines).wrap(Wrap { trim: false }), inner);
}

fn render_controls(f: &mut Frame, area: Rect, app: &App) {
    let controls = match app.run_state {
        RunState::Running => " [P] Pause  [Q] Quit  [↑↓] Scroll logs ",
        RunState::Paused => " [R] Resume  [Q] Quit  [↑↓] Scroll logs ",
        RunState::Finished | RunState::Cancelled | RunState::Error(_) => " [Q] Quit  [↑↓] Scroll logs ",
        _ => "",
    };
    let para = Paragraph::new(Line::from(Span::styled(
        controls,
        Style::default().fg(Color::DarkGray),
    )))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(para, area);
}
