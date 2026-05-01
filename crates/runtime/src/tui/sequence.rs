use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Gauge, Paragraph},
    Frame, Terminal,
};

use lodge_shared::placement::PlacementPlan;

use crate::engine::executor::{self, StepState};

use super::palette;

/// A single display step in the sequence screen.
pub struct Step {
    pub label: String,
    pub detail: String,
    pub state: StepState,
}

/// Builds the display step list from a [`PlacementPlan`].
pub fn steps_for_plan(plan: &PlacementPlan) -> Vec<Step> {
    let mut steps = Vec::new();

    for hook in plan.hooks_order.iter().filter(|h| h.contains("pre")) {
        steps.push(Step {
            label: "pre-install".into(),
            detail: hook.clone(),
            state: StepState::Pending,
        });
    }

    for entry in &plan.entries {
        let label = file_label(
            &entry
                .source
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        let detail = entry.destination.to_string_lossy().into_owned();
        steps.push(Step {
            label,
            detail,
            state: StepState::Pending,
        });
    }

    if plan.registrations.add_to_path {
        steps.push(Step {
            label: "PATH".into(),
            detail: "adding shim directory to PATH".into(),
            state: StepState::Pending,
        });
    }
    if let Some(ref var) = plan.registrations.env_var {
        steps.push(Step {
            label: "env var".into(),
            detail: var.clone(),
            state: StepState::Pending,
        });
    }
    if plan.registrations.start_menu_entry {
        steps.push(Step {
            label: "start menu".into(),
            detail: "creating start menu entry".into(),
            state: StepState::Pending,
        });
    }

    for hook in plan.hooks_order.iter().filter(|h| h.contains("post")) {
        steps.push(Step {
            label: "post-install".into(),
            detail: hook.clone(),
            state: StepState::Pending,
        });
    }

    steps
}

/// Runs the sequence screen: renders, executes each step, re-renders.
///
/// Polls for `q` or `Esc` between steps to allow abort.
pub fn run<B: ratatui::backend::Backend>(
    id: &str,
    version: &str,
    plan: &PlacementPlan,
    pkg_root: &std::path::Path,
    terminal: &mut Terminal<B>,
) -> anyhow::Result<Vec<String>> {
    let mut steps = steps_for_plan(plan);
    let total = steps.len();

    terminal.draw(|f| render(id, version, &steps, 0, total, f))?;

    let mut hooks_run = Vec::new();
    let mut done = 0usize;

    for i in 0..steps.len() {
        // Check for abort key between steps (non-blocking)
        if event::poll(std::time::Duration::from_millis(0))? {
            if let Event::Key(k) = event::read()? {
                if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }

        steps[i].state = StepState::InProgress;
        terminal.draw(|f| render(id, version, &steps, done, total, f))?;

        // Execute the actual step
        let result = execute_step(&steps[i], plan, pkg_root);
        match result {
            Ok(hook) => {
                if let Some(h) = hook {
                    hooks_run.push(h);
                }
                steps[i].state = StepState::Done;
                done += 1;
            }
            Err(e) => {
                steps[i].state = StepState::Failed(e.to_string());
                // Continue — record partial placements for receipt.
            }
        }
        terminal.draw(|f| render(id, version, &steps, done, total, f))?;
    }

    // Show completion, wait for keypress
    loop {
        terminal.draw(|f| render(id, version, &steps, done, total, f))?;
        if let Event::Key(k) = event::read()? {
            if matches!(k.code, KeyCode::Enter | KeyCode::Char('q') | KeyCode::Esc) {
                break;
            }
        }
    }

    Ok(hooks_run)
}

fn execute_step(
    step: &Step,
    plan: &PlacementPlan,
    pkg_root: &std::path::Path,
) -> anyhow::Result<Option<String>> {
    match step.label.as_str() {
        "pre-install" | "post-install" => {
            executor::run_hook(&step.detail, pkg_root)?;
            Ok(Some(step.detail.clone()))
        }
        "PATH" | "env var" | "start menu" => {
            // Registration side effects — noted in receipt; actual registration
            // deferred to the shim layer (M6).
            Ok(None)
        }
        _ => {
            // File placement — find the matching plan entry by destination
            let entry = plan
                .entries
                .iter()
                .find(|e| e.destination.to_string_lossy() == step.detail);
            if let Some(e) = entry {
                executor::place_file(e)?;
            }
            Ok(None)
        }
    }
}

fn render(id: &str, version: &str, steps: &[Step], done: usize, total: usize, frame: &mut Frame) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Length(1), // divider
            Constraint::Length(1), // blank + status line
            Constraint::Min(0),    // steps
            Constraint::Length(1), // divider
            Constraint::Length(1), // progress bar
        ])
        .split(area);

    // Header
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(id, Style::default().fg(palette::TEXT).bold()),
            Span::styled("  ", Style::default()),
            Span::styled(version, Style::default().fg(palette::TEXT_DIM)),
        ])),
        chunks[0],
    );

    // Dividers
    let div = "─".repeat(area.width as usize);
    for chunk_idx in [1, 4] {
        frame.render_widget(
            Paragraph::new(Span::styled(&div, Style::default().fg(palette::BORDER))),
            chunks[chunk_idx],
        );
    }

    // Status hint
    frame.render_widget(
        Paragraph::new(Span::styled(
            "  finding a place for everything…",
            Style::default().fg(palette::TEXT_DIM),
        )),
        chunks[2],
    );

    // Step list
    let step_lines: Vec<Line> = steps
        .iter()
        .map(|s| {
            let (symbol, sym_style) = state_symbol(&s.state);
            let detail_short = truncate(&s.detail, area.width.saturating_sub(20) as usize);
            Line::from(vec![
                Span::styled(format!("  {symbol}  "), sym_style),
                Span::styled(
                    format!("{:<14}", s.label),
                    Style::default().fg(palette::TEXT_DIM),
                ),
                Span::styled("→ ", Style::default().fg(palette::BORDER)),
                Span::styled(detail_short, Style::default().fg(palette::TEXT)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(step_lines), chunks[3]);

    // Progress gauge — multiply first to preserve precision before integer division.
    let pct = (done * 100).checked_div(total).unwrap_or(0) as u16;
    let label = format!("  {done} / {total}");
    frame.render_widget(
        Gauge::default()
            .label(label)
            .percent(pct)
            .gauge_style(Style::default().fg(palette::ACCENT).bg(palette::SURFACE))
            .style(Style::default().fg(palette::TEXT_DIM)),
        chunks[5],
    );
}

fn state_symbol(state: &StepState) -> (&'static str, Style) {
    match state {
        StepState::Pending => ("·", Style::default().fg(palette::TEXT_DIM)),
        StepState::InProgress => ("◐", Style::default().fg(palette::IN_PROGRESS)),
        StepState::Done => ("✔", Style::default().fg(palette::SUCCESS)),
        StepState::Failed(_) => ("✖", Style::default().fg(palette::ERROR)),
        StepState::Warning(_) => ("!", Style::default().fg(palette::WARNING)),
    }
}

fn file_label(filename: &str) -> String {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext.to_lowercase().as_str() {
        "exe" => "binary",
        "dll" | "so" | "dylib" => "library",
        "json" | "yaml" | "toml" | "ini" | "cfg" => "config",
        "ps1" | "psm1" | "psd1" => "powershell",
        "service" => "service",
        "ttf" | "otf" | "woff" | "woff2" => "font",
        _ => "file",
    }
    .into()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("…{}", &s[s.len().saturating_sub(max.saturating_sub(1))..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::placement::{PlacementEntry, PlacementPlan, RegistrationEffects};
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;

    fn empty_plan() -> PlacementPlan {
        PlacementPlan {
            entries: vec![],
            registrations: RegistrationEffects::default(),
            hooks_order: vec![],
            requires_elevation: false,
        }
    }

    fn plan_with_entry(dest: &str) -> PlacementPlan {
        PlacementPlan {
            entries: vec![PlacementEntry {
                source: PathBuf::from("bin/tool.exe"),
                destination: PathBuf::from(dest),
                rename: None,
            }],
            registrations: RegistrationEffects {
                add_to_path: true,
                ..Default::default()
            },
            hooks_order: vec!["post_install".into()],
            requires_elevation: false,
        }
    }

    #[test]
    fn steps_for_empty_plan_is_empty() {
        let plan = empty_plan();
        assert!(steps_for_plan(&plan).is_empty());
    }

    #[test]
    fn steps_for_plan_includes_file_path_and_registration() {
        let plan = plan_with_entry("C:\\Programs\\tool.exe");
        let steps = steps_for_plan(&plan);
        // Order: pre-hooks → entries → registrations → post-hooks
        // plan has: one entry + PATH + post-install hook = 3 steps
        assert_eq!(steps.len(), 3, "expected 3 steps, got: {:?}", steps.iter().map(|s| &s.label).collect::<Vec<_>>());
        let labels: Vec<&str> = steps.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"binary"), "expected binary step");
        assert!(labels.contains(&"PATH"), "expected PATH step");
        assert!(labels.contains(&"post-install"), "expected post-install step");
        // Entry comes before post-install hook
        let binary_idx = labels.iter().position(|&l| l == "binary").unwrap();
        let post_idx = labels.iter().position(|&l| l == "post-install").unwrap();
        assert!(binary_idx < post_idx, "binary entry should precede post-install hook");
    }

    #[test]
    fn progress_pct_is_correct_at_midpoint() {
        // Test the arithmetic directly: 3 of 5 = 60%
        let done = 3usize;
        let total = 5usize;
        let pct = (done * 100).checked_div(total).unwrap_or(0) as u16;
        assert_eq!(pct, 60);
    }

    #[test]
    fn progress_pct_does_not_panic_on_zero_total() {
        let done = 0usize;
        let total = 0usize;
        let pct = (done * 100).checked_div(total).unwrap_or(0) as u16;
        assert_eq!(pct, 0);
    }

    #[test]
    fn sequence_render_does_not_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let steps = vec![
            Step { label: "binary".into(), detail: "C:\\Programs\\tool.exe".into(), state: StepState::Done },
            Step { label: "PATH".into(), detail: "shim dir".into(), state: StepState::Pending },
        ];
        terminal
            .draw(|f| render("mytool", "1.0.0", &steps, 1, 2, f))
            .unwrap();
    }

    #[test]
    fn sequence_render_shows_package_id() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let steps = vec![Step {
            label: "binary".into(),
            detail: "dest/tool.exe".into(),
            state: StepState::InProgress,
        }];
        terminal
            .draw(|f| render("mypkg", "2.0.0", &steps, 0, 1, f))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("mypkg"));
        assert!(content.contains("2.0.0"));
    }

    #[test]
    fn file_label_classifies_extensions() {
        assert_eq!(file_label("tool.exe"), "binary");
        assert_eq!(file_label("helper.dll"), "library");
        assert_eq!(file_label("config.json"), "config");
        assert_eq!(file_label("module.psm1"), "powershell");
        assert_eq!(file_label("font.ttf"), "font");
        assert_eq!(file_label("readme.txt"), "file");
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string_adds_ellipsis() {
        let s = truncate("abcdefghij", 5);
        assert!(s.starts_with('…'));
        assert!(s.len() <= 5 + '…'.len_utf8());
    }
}
