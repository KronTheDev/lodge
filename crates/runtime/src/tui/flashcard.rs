use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame, Terminal,
};

use lodge_shared::{manifest::Manifest, placement::PlacementPlan};

use super::palette;

/// Displays the pre-install flashcard and waits for the user to confirm or cancel.
///
/// Returns `true` if the user pressed `I` (settle in), `false` if `C` or `Esc`.
pub fn show<B: ratatui::backend::Backend>(
    manifest: &Manifest,
    plan: &PlacementPlan,
    terminal: &mut Terminal<B>,
) -> anyhow::Result<bool> {
    loop {
        terminal.draw(|f| render(manifest, plan, f))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('i') | KeyCode::Char('I') | KeyCode::Enter => return Ok(true),
                KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => return Ok(false),
                _ => {}
            }
        }
    }
}

pub(crate) fn render(manifest: &Manifest, plan: &PlacementPlan, frame: &mut Frame) {
    let area = frame.area();

    // Centre a fixed-width card.
    let card_w = area.width.min(58);
    let card_h = area.height.min(24);
    let h_pad = (area.width.saturating_sub(card_w)) / 2;
    let v_pad = (area.height.saturating_sub(card_h)) / 2;

    let card_area = ratatui::layout::Rect {
        x: h_pad,
        y: v_pad,
        width: card_w,
        height: card_h,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(palette::BORDER))
        .style(Style::default().bg(palette::SURFACE));
    frame.render_widget(block, card_area);

    let inner = card_area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // id + version
            Constraint::Length(1), // author + type
            Constraint::Length(1), // blank
            Constraint::Length(1), // description
            Constraint::Length(1), // blank
            Constraint::Length(1), // divider
            Constraint::Length(1), // blank
            Constraint::Length(1), // installs as
            Constraint::Length(1), // scope
            Constraint::Length(1), // location
            Constraint::Length(1), // needs admin
            Constraint::Length(1), // hooks
            Constraint::Length(1), // blank
            Constraint::Length(1), // divider
            Constraint::Length(1), // blank
            Constraint::Length(1), // actions
        ])
        .split(inner);

    // Row 0: id + version
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(&manifest.id, Style::default().fg(palette::TEXT).bold()),
            Span::styled("  ", Style::default()),
            Span::styled(&manifest.version, Style::default().fg(palette::TEXT_DIM)),
        ])),
        rows[0],
    );

    // Row 1: author + type
    let type_str = format!("{:?}", manifest.package_type)
        .to_lowercase()
        .replace("_", "-");
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                manifest
                    .author
                    .as_deref()
                    .map(|a| format!("by {a}"))
                    .unwrap_or_default(),
                Style::default().fg(palette::TEXT_DIM),
            ),
            Span::styled(
                format!(
                    "{:>width$}",
                    type_str,
                    width = (inner.width as usize).saturating_sub(
                        manifest.author.as_deref().map(|a| a.len() + 3).unwrap_or(0)
                    )
                ),
                Style::default().fg(palette::ACCENT),
            ),
        ])),
        rows[1],
    );

    // Row 3: description — truncated to card width
    let desc_str = fit(manifest.description.as_deref().unwrap_or(""), inner.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(desc_str, Style::default().fg(palette::TEXT_DIM))),
        rows[3],
    );

    // Row 5 + 13: dividers
    let divider = "─".repeat(inner.width as usize);
    for row_idx in [5, 13] {
        frame.render_widget(
            Paragraph::new(Span::styled(&divider, Style::default().fg(palette::BORDER))),
            rows[row_idx],
        );
    }

    // Metadata rows
    let label_style = Style::default().fg(palette::TEXT_DIM);
    let value_style = Style::default().fg(palette::TEXT);

    let command_name = manifest.command_name().to_string();
    let scope_label = match manifest.preferred_scope() {
        lodge_shared::manifest::Scope::User => "current user",
        lodge_shared::manifest::Scope::System => "all users",
    };

    // Derive primary install location from first plan entry.
    // Shorten well-known Windows base paths to their env-var equivalents so the
    // path fits on one line even on machines with long usernames.
    let location_raw = plan
        .entries
        .first()
        .map(|e| {
            e.destination
                .parent()
                .unwrap_or(&e.destination)
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "—".into());
    let location = shorten_path(&location_raw);

    let hooks_label =
        if manifest.hooks.pre_install.is_some() && manifest.hooks.post_install.is_some() {
            "pre-install, post-install scripts"
        } else if manifest.hooks.pre_install.is_some() {
            "pre-install script"
        } else if manifest.hooks.post_install.is_some() {
            "post-install script"
        } else {
            "none"
        };

    // Label column is 14 chars; value gets the remainder.
    let val_w = (inner.width as usize).saturating_sub(14);

    let meta: [(&str, String); 5] = [
        ("installs as", fit(&command_name,                                  val_w)),
        ("scope",       scope_label.to_string()                                   ),
        ("location",    fit(&location,                                      val_w)),
        ("needs admin", if plan.requires_elevation { "yes" } else { "no" }.to_string()),
        ("hooks",       fit(hooks_label,                                    val_w)),
    ];

    for (i, (label, value)) in meta.iter().enumerate() {
        let row = rows[7 + i];
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{label:<14}"), label_style),
                Span::styled(value.clone(), value_style),
            ])),
            row,
        );
    }

    // Row 15: action keys
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[I]", Style::default().fg(palette::ACCENT).bold()),
            Span::styled(
                " settle in          ",
                Style::default().fg(palette::TEXT_DIM),
            ),
            Span::styled("[C]", Style::default().fg(palette::TEXT_DIM).bold()),
            Span::styled(" leave it", Style::default().fg(palette::TEXT_DIM)),
        ]))
        .alignment(Alignment::Center),
        rows[15],
    );
}

/// Truncate `s` to at most `max` display characters, appending `…` if cut.
fn fit(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let cut: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{cut}…")
    }
}

/// Replace long well-known path prefixes with their env-var equivalents.
///
/// `%LOCALAPPDATA%\Programs\mytool` is far more readable than
/// `C:\Users\andrew\AppData\Local\Programs\mytool` in the narrow flashcard.
fn shorten_path(path: &str) -> String {
    #[cfg(windows)]
    {
        for (var, label) in &[
            ("LOCALAPPDATA", "%LOCALAPPDATA%"),
            ("APPDATA",      "%APPDATA%"),
            ("USERPROFILE",  "~"),
        ] {
            if let Ok(val) = std::env::var(var) {
                if !val.is_empty() && path.starts_with(&val) {
                    return format!("{label}{}", &path[val.len()..]);
                }
            }
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::{
        manifest::{Manifest, PackageType, Prefers, Scope},
        placement::{PlacementPlan, RegistrationEffects},
    };
    use ratatui::{backend::TestBackend, Terminal};

    fn test_manifest(id: &str) -> Manifest {
        Manifest {
            id: id.to_string(),
            version: "1.0.0".into(),
            package_type: PackageType::CliTool,
            description: Some("A test package.".into()),
            author: Some("tester".into()),
            prefers: Prefers {
                scope: Some(Scope::User),
                ..Default::default()
            },
            requires: Default::default(),
            naming: Default::default(),
            overrides: vec![],
            hooks: Default::default(),
        }
    }

    fn empty_plan() -> PlacementPlan {
        PlacementPlan {
            entries: vec![],
            registrations: RegistrationEffects::default(),
            hooks_order: vec![],
            requires_elevation: false,
        }
    }

    #[test]
    fn flashcard_renders_without_panic() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let manifest = test_manifest("testpkg");
        let plan = empty_plan();
        terminal.draw(|f| render(&manifest, &plan, f)).unwrap();
    }

    #[test]
    fn flashcard_shows_package_id_and_version() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let manifest = test_manifest("mypkg");
        let plan = empty_plan();
        terminal.draw(|f| render(&manifest, &plan, f)).unwrap();

        let buf = terminal.backend().buffer().clone();
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("mypkg"), "package id not found in buffer");
        assert!(content.contains("1.0.0"), "version not found in buffer");
    }

    #[test]
    fn flashcard_shows_settle_in_prompt() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let manifest = test_manifest("testpkg");
        let plan = empty_plan();
        terminal.draw(|f| render(&manifest, &plan, f)).unwrap();

        let buf = terminal.backend().buffer().clone();
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("[I]"), "settle-in prompt not found");
        assert!(content.contains("[C]"), "leave-it prompt not found");
    }

    #[test]
    fn flashcard_no_admin_when_elevation_not_required() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let manifest = test_manifest("testpkg");
        let plan = empty_plan(); // requires_elevation = false
        terminal.draw(|f| render(&manifest, &plan, f)).unwrap();

        let buf = terminal.backend().buffer().clone();
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("no"), "expected 'no' for needs-admin field");
    }
}
