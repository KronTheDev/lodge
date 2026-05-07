// Extension browser overlay — opened via `!ext`.
//
// Two modes:
//   List — Up/Down to select extension, Enter to open detail, Q/Esc to close.
//   Detail — Left/Right (or Tab) to cycle sub-pages, Esc/B to return to list.
//
// Sub-pages per extension:
//   0  overview  — name, version, description, status, command
//   1  commands  — list of sub-commands (from RegistryEntry.commands)
//
// All dynamic text is clipped to the enclosing Rect width before rendering.

use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::engine::extensions::RegistryEntry;
use super::palette;

// ── State ─────────────────────────────────────────────────────────────────────

/// Mutable state for the extension browser overlay.
pub struct ExtBrowserState {
    pub entries: Vec<RegistryEntry>,
    /// Currently highlighted extension index in the list.
    pub selected: usize,
    /// `true` = detail view is open; `false` = list view.
    pub in_detail: bool,
    /// Current sub-page index in detail view.
    pub sub_page: usize,
}

impl ExtBrowserState {
    pub fn new(entries: Vec<RegistryEntry>) -> Self {
        Self { entries, selected: 0, in_detail: false, sub_page: 0 }
    }

    /// Number of sub-pages for the currently selected entry.
    pub fn sub_page_count(&self) -> usize {
        let e = &self.entries[self.selected];
        if e.commands.is_empty() { 1 } else { 2 }
    }
}

// ── Fit helper ────────────────────────────────────────────────────────────────

/// Truncate `s` to `max` visible characters, appending `…` if clipped.
/// `max = 0` returns an empty string.
fn fit(s: &str, max: usize) -> String {
    if max == 0 { return String::new(); }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        chars[..max.saturating_sub(1)].iter().collect::<String>() + "…"
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

/// Render the extension browser overlay into the current frame.
pub fn render(state: &ExtBrowserState, frame: &mut Frame) {
    let area = frame.area();

    let card_w = 72u16.min(area.width.saturating_sub(4));
    let card_h = 28u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(card_w)) / 2;
    let y = area.y + (area.height.saturating_sub(card_h)) / 2;
    let card = Rect { x, y, width: card_w, height: card_h };

    frame.render_widget(Clear, card);

    let title_text = if state.in_detail {
        // Fit title to card width minus borders and padding.
        let max_title = card_w.saturating_sub(6) as usize;
        format!(" {} ", fit(&state.entries[state.selected].name, max_title))
    } else {
        " extensions ".to_string()
    };
    let title_line = Line::from(Span::styled(title_text, Style::default().fg(palette::ACCENT)));

    let block = Block::default()
        .title(title_line)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(palette::BORDER))
        .style(Style::default().bg(palette::SURFACE));

    let inner = block.inner(card);
    frame.render_widget(block, card);

    // Bottom 2 rows: separator + nav bar.
    let content_h = inner.height.saturating_sub(2);
    let content_area = Rect { height: content_h, ..inner };
    let sep_area = Rect { x: inner.x, y: inner.y + content_h,     width: inner.width, height: 1 };
    let nav_area = Rect { x: inner.x, y: inner.y + content_h + 1, width: inner.width, height: 1 };

    // Separator — always fills exactly the inner width.
    let sep = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(sep, Style::default().fg(palette::BORDER)))),
        sep_area,
    );

    frame.render_widget(
        Paragraph::new(nav_line(state)).alignment(Alignment::Center),
        nav_area,
    );

    if state.in_detail {
        render_detail(state, frame, content_area);
    } else {
        render_list(state, frame, content_area);
    }
}

// ── List view ─────────────────────────────────────────────────────────────────

fn render_list(state: &ExtBrowserState, frame: &mut Frame, area: Rect) {
    let padded = area.inner(Margin { horizontal: 2, vertical: 1 });
    let w = padded.width as usize;

    let mut lines: Vec<Line<'_>> = vec![
        Line::from(Span::styled(
            format!("  {} extension{} available", state.entries.len(),
                if state.entries.len() == 1 { "" } else { "s" }),
            Style::default().fg(palette::TEXT_DIM),
        )),
        Line::from(""),
    ];

    for (i, entry) in state.entries.iter().enumerate() {
        let selected = i == state.selected;

        let indicator = if selected {
            Span::styled("▶ ", Style::default().fg(palette::ACCENT))
        } else {
            Span::raw("  ")
        };

        let name_style = if selected {
            Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette::TEXT)
        };
        let alias_style = if selected {
            Style::default().fg(palette::ACCENT)
        } else {
            Style::default().fg(palette::TEXT_DIM)
        };
        let status_style = Style::default().fg(match entry.status.as_str() {
            "stable"  => palette::SUCCESS,
            "preview" => palette::WARNING,
            _         => palette::TEXT_DIM,
        });

        // Right side of name row: "  v{version}  !{alias}  [{status}]"
        // Reserve space for these fields so name doesn't collide.
        let alias    = format!("!{}", entry.command_alias());
        let version  = format!("v{}", entry.version);
        let status   = format!("[{}]", entry.status);
        // fixed overhead: indicator(2) + "  "(2) + "  "(2) + "  "(2) = 8
        // plus the version, alias, status widths
        let right_w  = 2 + version.len() + 2 + alias.len() + 2 + status.len();
        let name_max = w.saturating_sub(2 + right_w); // 2 = indicator width
        let name     = fit(&entry.name, name_max);

        // Name row — all pieces on one line.
        lines.push(Line::from(vec![
            indicator.clone(),
            Span::styled(name, name_style),
            Span::raw("  "),
            Span::styled(version, Style::default().fg(palette::TEXT_DIM)),
            Span::raw("  "),
            Span::styled(alias, alias_style),
            Span::raw("  "),
            Span::styled(status, status_style),
        ]));

        // Description row — indented, clipped to panel width.
        let indent = 2usize; // "  "
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(fit(&entry.description, w.saturating_sub(indent)),
                Style::default().fg(palette::TEXT_DIM)),
        ]));

        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines), padded);
}

// ── Detail view ───────────────────────────────────────────────────────────────

fn render_detail(state: &ExtBrowserState, frame: &mut Frame, area: Rect) {
    let entry = &state.entries[state.selected];
    let padded = area.inner(Margin { horizontal: 2, vertical: 1 });

    let total = state.sub_page_count();
    let dots_area = Rect { height: 1, ..padded };
    let content_area = Rect {
        y: padded.y + 2,
        height: padded.height.saturating_sub(2),
        ..padded
    };

    if total > 1 {
        let mut dots: Vec<Span<'_>> = Vec::new();
        for i in 0..total {
            if i > 0 { dots.push(Span::raw("  ")); }
            if i == state.sub_page {
                dots.push(Span::styled("●", Style::default().fg(palette::ACCENT)));
            } else {
                dots.push(Span::styled("○", Style::default().fg(palette::TEXT_DIM)));
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(dots)).alignment(Alignment::Center),
            dots_area,
        );
    }

    let w = padded.width as usize;
    let lines: Vec<Line<'_>> = match state.sub_page {
        0 => detail_overview(entry, w),
        1 => detail_commands(entry, w),
        _ => vec![],
    };

    frame.render_widget(Paragraph::new(lines), content_area);
}

fn detail_overview(entry: &RegistryEntry, w: usize) -> Vec<Line<'_>> {
    // Label column is 14 chars; value gets the rest.
    const LABEL_W: usize = 14;
    let val_w = w.saturating_sub(LABEL_W);

    let lbl = |k: &'static str| Span::styled(format!("{k:<LABEL_W$}"), Style::default().fg(palette::TEXT_DIM));
    let val = |v: String| Span::styled(v, Style::default().fg(palette::TEXT));

    // Name + version on one line; name gets available width after version.
    let version  = format!("v{}", entry.version);
    let name_max = w.saturating_sub(2 + version.len()); // 2 = "  " separator
    let name     = fit(&entry.name, name_max);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(name, Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(version, Style::default().fg(palette::TEXT_DIM)),
        ]),
        Line::from(""),
        // Description — full panel width.
        Line::from(Span::styled(fit(&entry.description, w), Style::default().fg(palette::TEXT_DIM))),
        Line::from(""),
        Line::from(Span::styled("─".repeat(w), Style::default().fg(palette::BORDER))),
        Line::from(""),
        Line::from(vec![
            lbl("command"),
            val(fit(&format!("!{}", entry.command_alias()), val_w)),
        ]),
        Line::from(vec![
            lbl("status"),
            val(fit(&entry.status, val_w)),
        ]),
    ];

    if let Some(url) = &entry.payload_url {
        if !url.is_empty() {
            lines.push(Line::from(vec![
                lbl("source"),
                val(fit(url, val_w)),
            ]));
        }
    }

    let verified = entry.sha256.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
    lines.push(Line::from(vec![
        lbl("sha-256"),
        Span::styled(
            if verified { "verified" } else { "unverified" },
            Style::default().fg(if verified { palette::SUCCESS } else { palette::WARNING }),
        ),
    ]));

    lines
}

fn detail_commands(entry: &RegistryEntry, w: usize) -> Vec<Line<'_>> {
    if entry.commands.is_empty() {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                fit("  no command list — run !<alias> help for details", w),
                Style::default().fg(palette::TEXT_DIM),
            )),
        ];
    }

    // Usage column: 22 chars + 2 indent = 24 reserved; description gets rest.
    const USAGE_W: usize  = 22;
    const INDENT: usize   = 2;
    let desc_w = w.saturating_sub(INDENT + USAGE_W);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  commands", Style::default().fg(palette::TEXT_DIM))),
        Line::from(""),
    ];

    for cmd in &entry.commands {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<USAGE_W$}", fit(&cmd.usage, USAGE_W)),
                Style::default().fg(palette::ACCENT),
            ),
            Span::styled(
                fit(&cmd.description, desc_w),
                Style::default().fg(palette::TEXT_DIM),
            ),
        ]));
    }

    lines
}

// ── Nav bar ───────────────────────────────────────────────────────────────────

fn nav_line(state: &ExtBrowserState) -> Line<'_> {
    let dim = Style::default().fg(palette::TEXT_DIM);

    if state.in_detail {
        let total = state.sub_page_count();
        let mut spans = vec![Span::styled("[←][→] pages  ", dim)];
        if total > 1 {
            for i in 0..total {
                if i > 0 { spans.push(Span::raw("  ")); }
                if i == state.sub_page {
                    spans.push(Span::styled("●", Style::default().fg(palette::ACCENT)));
                } else {
                    spans.push(Span::styled("○", dim));
                }
            }
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled("[B] back  [Q] close", dim));
        Line::from(spans)
    } else {
        Line::from(Span::styled("[↑][↓] select  [Enter] open  [Q] close", dim))
    }
}
