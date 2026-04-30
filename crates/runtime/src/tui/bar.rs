use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};

use super::{palette, splash};

/// Runs the interactive command bar.
///
/// Shows the splash screen, then opens a persistent `> _` prompt.
/// Input is read line by line. The brain integration (M5) will route commands;
/// for now, a stub response is returned for every input.
pub fn run() -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    // Restore terminal on panic
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stderr(), LeaveAlternateScreen);
        original(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Show splash screen until any key is pressed
    loop {
        terminal.draw(splash::render)?;
        if let Event::Key(_) = event::read()? {
            break;
        }
    }

    // Command bar loop
    let mut input = String::new();
    let mut history: Vec<(String, String)> = Vec::new(); // (input, response)

    loop {
        terminal.draw(|f| render_bar(&input, &history, f))?;

        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => break,

                (KeyCode::Enter, _) => {
                    let trimmed = input.trim().to_string();
                    if !trimmed.is_empty() {
                        let response = handle_command(&trimmed);
                        history.push((trimmed, response));
                        // Keep only last 6 exchanges in display
                        if history.len() > 6 {
                            history.remove(0);
                        }
                    }
                    input.clear();
                }

                (KeyCode::Backspace, _) => {
                    input.pop();
                }

                (KeyCode::Char(c), _) => {
                    input.push(c);
                }

                _ => {}
            }
        }
    }

    // Restore
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn render_bar(input: &str, history: &[(String, String)], frame: &mut ratatui::Frame) {
    let area = frame.area();
    let div = "─".repeat(area.width as usize);

    // Count history lines needed
    let history_lines: Vec<Line> = history
        .iter()
        .flat_map(|(cmd, resp)| {
            let mut lines = vec![Line::from(vec![
                Span::styled("  > ", Style::default().fg(palette::ACCENT)),
                Span::styled(cmd.clone(), Style::default().fg(palette::TEXT)),
            ])];
            if !resp.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("  {resp}"),
                    Style::default().fg(palette::TEXT_DIM),
                )));
                lines.push(Line::from(""));
            }
            lines
        })
        .collect();

    let history_height = history_lines.len() as u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),                // spacer / history
            Constraint::Length(1),             // divider
            Constraint::Length(1),             // input line
            Constraint::Length(1),             // divider
        ])
        .split(area);

    // History
    if history_height > 0 {
        let hist_area = ratatui::layout::Rect {
            x: chunks[0].x,
            y: chunks[0].bottom().saturating_sub(history_height),
            width: chunks[0].width,
            height: history_height.min(chunks[0].height),
        };
        frame.render_widget(Paragraph::new(history_lines), hist_area);
    }

    // Dividers
    let div_span = Span::styled(&div, Style::default().fg(palette::BORDER));
    frame.render_widget(Paragraph::new(Line::from(div_span.clone())), chunks[1]);
    frame.render_widget(Paragraph::new(Line::from(div_span)), chunks[3]);

    // Input line with cursor
    let prompt = Line::from(vec![
        Span::styled("  > ", Style::default().fg(palette::ACCENT)),
        Span::styled(input, Style::default().fg(palette::TEXT)),
        Span::styled("_", Style::default().fg(palette::HIGHLIGHT)),
    ]);
    frame.render_widget(Paragraph::new(prompt), chunks[2]);
}

/// Stub command handler — replaced by brain integration in M5.
fn handle_command(input: &str) -> String {
    match input.trim().to_lowercase().as_str() {
        "help" => "commands: install, list, info, verify, history, help".into(),
        "list" => "no packages installed yet.".into(),
        "history" => "no installation history.".into(),
        cmd if cmd.starts_with("install ") => {
            "use `lodge install <path>` from the terminal to install packages.".to_string()
        }
        _ => "brain not yet connected — M5 will route this.".into(),
    }
}
