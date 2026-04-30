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

use lodge_brain::{Brain, Command};

use super::{palette, splash};
use crate::engine::attester;

/// Runs the interactive command bar.
///
/// Shows the splash screen, then opens a persistent `> _` prompt.
/// Input is routed through the brain (deterministic resolver + model if loaded).
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

    // Initialise brain (tries to load model, degrades gracefully)
    let mut brain = Brain::new();
    let model_note = if brain.has_model() {
        None
    } else {
        Some("running in deterministic mode — place smollm2-360m-q4_k_m.gguf alongside the binary to enable AI.")
    };

    let mut input = String::new();
    let mut history: Vec<(String, String)> = Vec::new();

    // Show model note as first history entry if applicable
    if let Some(note) = model_note {
        history.push((String::new(), note.to_string()));
    }

    loop {
        terminal.draw(|f| render_bar(&input, &history, f))?;

        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => break,

                (KeyCode::Enter, _) => {
                    let trimmed = input.trim().to_string();
                    if !trimmed.is_empty() {
                        let response = handle_command(&mut brain, &trimmed);
                        history.push((trimmed, response));
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

/// Routes a command through the brain, with runtime-layer overrides for
/// commands that need access to the local filesystem (history, list).
fn handle_command(brain: &mut Brain, input: &str) -> String {
    // Resolve intent first to check the command type
    let intent = lodge_brain::intent::resolve_deterministic(input);
    match intent.command {
        Command::History => format_history(),
        Command::List => format_installed(),
        _ => brain.handle(input),
    }
}

/// Reads receipts from disk and formats them as an installation history.
fn format_history() -> String {
    let receipts = attester::list_receipts();
    if receipts.is_empty() {
        return "no installation history.".into();
    }
    receipts
        .iter()
        .take(10)
        .map(|r| format!("  {}  v{}  ({})", r.id, r.version, &r.installed_at[..10]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Reads receipts to determine what's currently installed.
fn format_installed() -> String {
    let receipts = attester::list_receipts();
    if receipts.is_empty() {
        return "no packages installed yet.".into();
    }
    // Deduplicate by id, keeping newest
    let mut seen = std::collections::HashSet::new();
    let lines: Vec<String> = receipts
        .into_iter()
        .filter(|r| seen.insert(r.id.clone()))
        .map(|r| format!("  {}  v{}", r.id, r.version))
        .collect();
    lines.join("\n")
}

fn render_bar(input: &str, history: &[(String, String)], frame: &mut ratatui::Frame) {
    let area = frame.area();
    let div = "─".repeat(area.width as usize);

    // Build history lines
    let history_lines: Vec<Line> = history
        .iter()
        .flat_map(|(cmd, resp)| {
            let mut lines: Vec<Line> = Vec::new();
            // Only show the prompt line if there was actual input
            if !cmd.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  > ", Style::default().fg(palette::ACCENT)),
                    Span::styled(cmd.clone(), Style::default().fg(palette::TEXT)),
                ]));
            }
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
            Constraint::Min(0),    // spacer / history
            Constraint::Length(1), // divider
            Constraint::Length(1), // input line
            Constraint::Length(1), // divider
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
