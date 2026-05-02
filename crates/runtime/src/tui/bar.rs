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
/// commands that need filesystem access or shim manipulation.
fn handle_command(brain: &mut Brain, input: &str) -> String {
    let intent = lodge_brain::intent::resolve_deterministic(input);
    match intent.command {
        Command::History => format_history(),
        Command::List => format_installed(),

        Command::Uninstall => {
            let id = intent.args["id"].as_str().unwrap_or("").trim().to_string();
            if id.is_empty() {
                "uninstall what? try: uninstall <id>".into()
            } else {
                run_uninstall(&id)
            }
        }

        Command::Verify => {
            let id = intent.args["id"].as_str().unwrap_or("").trim().to_string();
            if id.is_empty() {
                "verify what? try: verify <id>".into()
            } else {
                run_verify(&id)
            }
        }

        Command::Info => {
            let id = intent.args["id"].as_str().unwrap_or("").trim().to_string();
            if id.is_empty() {
                "info what? try: info <id>".into()
            } else {
                run_info(&id)
            }
        }

        Command::Use => {
            let spec = intent.args["spec"].as_str().unwrap_or("").trim().to_string();
            if spec.is_empty() {
                "use what? try: use <id>@<version>".into()
            } else {
                run_use(&spec)
            }
        }

        Command::Search => {
            let query = intent.args["query"].as_str().unwrap_or("").trim().to_string();
            if query.is_empty() {
                run_search_all()
            } else {
                run_search(&query)
            }
        }

        Command::Update => {
            let id = intent.args["id"].as_str().unwrap_or("").trim().to_string();
            if id.is_empty() {
                "update what? try: update <id> or update all".into()
            } else {
                run_update(&id)
            }
        }

        Command::UpdateAll => run_update_all(),

        Command::Rollback => {
            let id = intent.args["id"].as_str().unwrap_or("").trim().to_string();
            if id.is_empty() {
                "rollback what? try: rollback <id>".into()
            } else {
                run_rollback(&id)
            }
        }

        Command::Install => {
            let target = intent
                .args
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or(input.trim_start_matches("install").trim())
                .trim()
                .to_string();
            if target.is_empty() {
                "install what? try: install <id> or lodge install <path>".into()
            } else {
                run_install_from_feed(&target)
            }
        }

        Command::UpdateRulesets => {
            "lodge ships with built-in rulesets for Windows, macOS, and Linux. \
             community ruleset updates are not yet available — check back in a future release."
                .into()
        }

        _ => brain.handle(input),
    }
}

/// Lists all packages in the local feed.
fn run_search_all() -> String {
    let results = crate::engine::feed::scan();
    crate::engine::feed::format_search_results(&results)
}

/// Searches the local feed and formats matching entries.
fn run_search(query: &str) -> String {
    let results = crate::engine::feed::search(query);
    crate::engine::feed::format_search_results(&results)
}

/// Installs a package from the local feed by id (bar-only: no TUI, engine-only).
fn run_install_from_feed(target: &str) -> String {
    // In the bar context we can't spawn a full TUI install — resolve from feed
    // and report where to find it, or do a silent install for feed packages.
    match crate::engine::feed::find_latest(target) {
        None => format!(
            "'{target}' not found in the local feed. \
             use `lodge install {target}` from the terminal for path-based installs."
        ),
        Some(entry) => {
            match crate::engine::installer::silent_install(&entry.path, lodge::VERSION) {
                Ok(receipt) => format!(
                    "{} v{} settled in.",
                    receipt.id, receipt.version
                ),
                Err(e) => format!("couldn't install {target}: {e}"),
            }
        }
    }
}

/// Updates a package from the local feed.
fn run_update(id: &str) -> String {
    match crate::engine::update::update(id, lodge::VERSION) {
        Ok(result) => crate::engine::update::format_update_result(id, &result),
        Err(e) => format!("couldn't update {id}: {e}"),
    }
}

/// Updates all installed packages from the local feed.
fn run_update_all() -> String {
    let results = crate::engine::update::update_all(lodge::VERSION);
    if results.is_empty() {
        return "no packages installed.".into();
    }
    results
        .iter()
        .map(|(id, result)| match result {
            Ok(r) => crate::engine::update::format_update_result(id, r),
            Err(e) => format!("{id}: {e}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rolls back a package to its previous version.
fn run_rollback(id: &str) -> String {
    match crate::engine::rollback::rollback(id, lodge::VERSION) {
        Ok(result) => crate::engine::rollback::format_rollback_result(id, &result),
        Err(e) => format!("couldn't roll back {id}: {e}"),
    }
}

/// Shows details for an installed package from its most recent receipt.
fn run_info(id: &str) -> String {
    let receipts = attester::list_receipts();
    match receipts.into_iter().find(|r| r.id == id) {
        None => format!("{id} is not installed."),
        Some(r) => {
            let date = if r.installed_at.len() >= 10 {
                &r.installed_at[..10]
            } else {
                &r.installed_at
            };
            let mut lines = vec![
                format!("{}  v{}", r.id, r.version),
                format!("  installed  {date}"),
                format!("  scope      {}", r.scope),
                format!("  files      {}", r.placements.len()),
            ];
            for p in &r.placements {
                let name = std::path::Path::new(&p.destination)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&p.destination);
                lines.push(format!("    → {name}"));
            }
            lines.join("\n")
        }
    }
}

/// Switches the active version shim and returns a plain-language result.
fn run_use(spec: &str) -> String {
    let Some(at) = spec.rfind('@') else {
        return format!("invalid spec — expected id@version, got '{spec}'");
    };
    let (id, version) = (&spec[..at], &spec[at + 1..]);

    let receipts = attester::list_receipts();
    let Some(receipt) = receipts
        .into_iter()
        .find(|r| r.id == id && r.version.starts_with(version))
    else {
        return format!(
            "{id} v{version} is not installed. use `list` to see what's installed."
        );
    };

    let Some(placed) = receipt.placements.first() else {
        return format!("no placed files in receipt for {id}.");
    };

    let target = std::path::Path::new(&placed.destination);
    match crate::shim::register::update(id, target) {
        Ok(_) => format!("shim updated — {id} now resolves to v{}.", receipt.version),
        Err(e) => format!("couldn't update shim: {e}"),
    }
}

/// Uninstalls a package and returns a plain-language result.
fn run_uninstall(id: &str) -> String {
    match crate::engine::uninstall::uninstall(id) {
        Ok(result) => {
            let mut lines = vec![format!("{id} removed.")];
            if !result.missing_files.is_empty() {
                lines.push(format!(
                    "  {} file(s) were already gone.",
                    result.missing_files.len()
                ));
            }
            if result.shim_removed {
                lines.push("  shim unregistered.".into());
            }
            lines.join("\n")
        }
        Err(e) => format!("couldn't uninstall {id}: {e}"),
    }
}

/// Verifies an installation and returns a plain-language result.
fn run_verify(id: &str) -> String {
    match crate::engine::verify::verify(id) {
        Ok(result) => crate::engine::verify::format_verify_result(&result),
        Err(e) => format!("couldn't verify {id}: {e}"),
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
