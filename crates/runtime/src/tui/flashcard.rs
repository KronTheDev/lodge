use crossterm::event::{self, Event, KeyCode};
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
            match key.code {
                KeyCode::Char('i') | KeyCode::Char('I') | KeyCode::Enter => return Ok(true),
                KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => return Ok(false),
                _ => {}
            }
        }
    }
}

fn render(manifest: &Manifest, plan: &PlacementPlan, frame: &mut Frame) {
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

    // Row 3: description
    frame.render_widget(
        Paragraph::new(Span::styled(
            manifest.description.as_deref().unwrap_or(""),
            Style::default().fg(palette::TEXT_DIM),
        )),
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

    // Derive primary install location from first plan entry
    let location = plan
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

    let meta = [
        ("installs as", command_name.as_str()),
        ("scope", scope_label),
        ("location", &location),
        (
            "needs admin",
            if plan.requires_elevation { "yes" } else { "no" },
        ),
        ("hooks", hooks_label),
    ];

    for (i, (label, value)) in meta.iter().enumerate() {
        let row = rows[7 + i];
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{label:<14}"), label_style),
                Span::styled(*value, value_style),
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
