use crate::tui::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(14),   // Form
            Constraint::Length(3), // Error / status
            Constraint::Length(3),  // Help
        ])
        .split(area);

    render_title(f, chunks[0]);
    render_form(f, chunks[1], app);
    render_error(f, chunks[2], app);
    render_help(f, chunks[3], app);
}

fn render_title(f: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![
        Span::styled(" PGN Player Extractor ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" — Configuration", Style::default().fg(Color::DarkGray)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    f.render_widget(title, area);
}

fn render_form(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Settings ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    let field_count = app.fields.len();
    let mut constraints: Vec<Constraint> = app.fields.iter().map(|_| Constraint::Length(1)).collect();
    constraints.push(Constraint::Length(1)); // spacer
    constraints.push(Constraint::Length(1)); // start button
    constraints.push(Constraint::Min(0));   // filler

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(constraints)
        .split(inner);

    let label_width = 18u16;

    for (i, field) in app.fields.iter().enumerate() {
        let selected = app.selected == i;
        let editing = selected && app.editing;

        let label_style = if selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        let value_style = if editing {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        } else if selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        let hint_style = Style::default().fg(Color::DarkGray);

        let cursor_char = if editing { "▏" } else { "" };

        let value_display = if editing {
            let (before, after) = field.value.split_at(app.edit_cursor.min(field.value.len()));
            format!("{}{}{}", before, cursor_char, after)
        } else {
            field.value.clone()
        };

        let line = Line::from(vec![
            Span::styled(
                format!("{:>width$} │ ", field.label, width = label_width as usize),
                label_style,
            ),
            Span::styled(value_display, value_style),
            Span::styled(format!("  {}", field.hint), hint_style),
        ]);

        f.render_widget(Paragraph::new(line), rows[i]);
    }

    // Start button
    let btn_idx = field_count + 1; // after spacer
    let selected = app.is_on_start_button();
    let btn_style = if selected {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let arrow = if selected { "▶ " } else { "  " };
    let line = Line::from(vec![
        Span::raw("                   "),
        Span::styled(format!("{}[ Start Processing ]", arrow), btn_style),
    ]);
    if btn_idx < rows.len() {
        f.render_widget(Paragraph::new(line), rows[btn_idx]);
    }
}

fn render_error(f: &mut Frame, area: Rect, app: &App) {
    let msg = if let Some(err) = &app.validation_error {
        Line::from(Span::styled(
            format!(" ⚠ {}", err),
            Style::default().fg(Color::Red),
        ))
    } else {
        Line::from(Span::styled(
            " Ready to configure and start.",
            Style::default().fg(Color::DarkGray),
        ))
    };
    let para = Paragraph::new(msg)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(para, area);
}

fn render_help(f: &mut Frame, area: Rect, app: &App) {
    let help_text = if app.editing {
        " Type to edit │ Enter: Confirm │ Esc: Cancel "
    } else {
        " ↑↓: Navigate │ Enter: Edit/Start │ Tab: Next │ q: Quit "
    };
    let help = Paragraph::new(Line::from(Span::styled(
        help_text,
        Style::default().fg(Color::DarkGray),
    )))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(help, area);
}
