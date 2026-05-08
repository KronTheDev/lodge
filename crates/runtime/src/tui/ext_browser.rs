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
// `selected` is an index into the *visible* (filtered) list, not into `entries`.
// Use `selected_real_idx()` to get the corresponding `entries` position.
// All dynamic text is clipped to the enclosing Rect width before rendering.

use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::engine::extensions::{is_installed, RegistryEntry};
use super::palette;

// ── Action ────────────────────────────────────────────────────────────────────

/// An action the user triggered inside the browser that bar.rs must execute.
pub enum ExtAction {
    /// Toggle the extension on/off.  `bar.rs` persists the new state and
    /// auto-installs if the extension was just enabled but is not present.
    Toggle(String),
    /// Explicitly download and install the extension binary.
    Install(String),
    /// Remove the extension binary (and disable it if enabled).
    Uninstall(String),
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Mutable state for the extension browser overlay.
pub struct ExtBrowserState {
    pub entries: Vec<RegistryEntry>,
    /// Index into the *visible* (search-filtered) list.
    pub selected: usize,
    /// `true` = detail view is open; `false` = list view.
    pub in_detail: bool,
    /// Current sub-page index in detail view.
    pub sub_page: usize,
    /// IDs of extensions the user has explicitly enabled.
    pub enabled: std::collections::HashSet<String>,
    /// Precomputed installation status, indexed by `entries` position.
    pub installed: Vec<bool>,
    /// Whether the registry was fetched fresh from GitHub this session.
    pub online: bool,
    /// Current search / filter text.
    pub search: String,
    /// `true` while the search input is focused (typing goes to filter).
    pub search_active: bool,
    /// IDs designated official by the Lodge maintainers (from `official.json`).
    pub official_ids: std::collections::HashSet<String>,
}

impl ExtBrowserState {
    /// Create a new browser state.
    /// `enabled`      — persisted `ExtState.enabled`;
    /// `online`       — whether `fetch_registry` reached GitHub;
    /// `official_ids` — set fetched from `extensions/official.json`.
    pub fn new(
        entries: Vec<RegistryEntry>,
        enabled: std::collections::HashSet<String>,
        online: bool,
        official_ids: std::collections::HashSet<String>,
    ) -> Self {
        let installed = entries.iter().map(|e| is_installed(&e.id)).collect();
        Self {
            entries,
            selected: 0,
            in_detail: false,
            sub_page: 0,
            enabled,
            installed,
            online,
            search: String::new(),
            search_active: false,
            official_ids,
        }
    }

    /// Recompute `installed` from the filesystem.  Call after install/uninstall.
    pub fn refresh_installed(&mut self) {
        self.installed = self.entries.iter().map(|e| is_installed(&e.id)).collect();
    }

    /// Indices into `entries` that match the current search filter.
    pub fn visible_indices(&self) -> Vec<usize> {
        if self.search.is_empty() {
            return (0..self.entries.len()).collect();
        }
        let q = self.search.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.command_alias().to_lowercase().contains(&q)
                    || e.id.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Number of currently visible entries.
    pub fn visible_count(&self) -> usize {
        self.visible_indices().len()
    }

    /// The `entries` index of the currently selected visible item.
    pub fn selected_real_idx(&self) -> Option<usize> {
        self.visible_indices().get(self.selected).copied()
    }

    /// Number of sub-pages for the currently selected entry.
    pub fn sub_page_count(&self) -> usize {
        let Some(real) = self.selected_real_idx() else { return 1 };
        if self.entries[real].commands.is_empty() { 1 } else { 2 }
    }
}

// ── Fit helper ────────────────────────────────────────────────────────────────

/// Truncate `s` to `max` visible characters, appending `…` if clipped.
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
        let max_title = card_w.saturating_sub(6) as usize;
        if let Some(real) = state.selected_real_idx() {
            format!(" {} ", fit(&state.entries[real].name, max_title))
        } else {
            " extensions ".to_string()
        }
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
    let sep_area    = Rect { x: inner.x, y: inner.y + content_h,     width: inner.width, height: 1 };
    let nav_area    = Rect { x: inner.x, y: inner.y + content_h + 1, width: inner.width, height: 1 };

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

/// Append one entry row (name line + description line + blank) to `lines`.
fn emit_entry<'a>(
    state: &'a ExtBrowserState,
    real_idx: usize,
    vis_pos: usize,
    w: usize,
    lines: &mut Vec<Line<'a>>,
) {
    let entry     = &state.entries[real_idx];
    let selected  = vis_pos == state.selected;
    let on        = state.enabled.contains(&entry.id);
    let installed = state.installed.get(real_idx).copied().unwrap_or(false);
    let official  = state.official_ids.contains(&entry.id);

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
    let (toggle_text, toggle_style) = if on {
        let colour = if installed { palette::SUCCESS } else { palette::WARNING };
        ("[on]", Style::default().fg(colour))
    } else {
        ("[off]", Style::default().fg(palette::TEXT_DIM))
    };

    let alias        = format!("!{}", entry.command_alias());
    let version      = format!("v{}", entry.version);
    let status       = format!("[{}]", entry.status);
    let official_tag = if official { "  ✦" } else { "" };
    let right_w = 2 + version.len() + 2 + alias.len() + 2 + status.len()
                  + 2 + toggle_text.len() + official_tag.len();
    let name_max = w.saturating_sub(2 + right_w);
    let name     = fit(&entry.name, name_max);

    let mut spans: Vec<Span<'_>> = vec![
        indicator,
        Span::styled(name, name_style),
        Span::raw("  "),
        Span::styled(version, Style::default().fg(palette::TEXT_DIM)),
        Span::raw("  "),
        Span::styled(alias, alias_style),
        Span::raw("  "),
        Span::styled(status, status_style),
        Span::raw("  "),
        Span::styled(toggle_text, toggle_style),
    ];
    if official {
        spans.push(Span::styled(
            official_tag,
            Style::default().fg(palette::HIGHLIGHT),
        ));
    }
    lines.push(Line::from(spans));

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            fit(&entry.description, w.saturating_sub(2)),
            Style::default().fg(palette::TEXT_DIM),
        ),
    ]));

    lines.push(Line::from(""));
}

fn render_list(state: &ExtBrowserState, frame: &mut Frame, area: Rect) {
    let padded = area.inner(Margin { horizontal: 2, vertical: 1 });
    let w      = padded.width as usize;
    let vis    = state.visible_indices();

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Offline disclaimer
    if !state.online {
        lines.push(Line::from(Span::styled(
            fit("  ! offline — showing cached registry", w),
            Style::default().fg(palette::WARNING),
        )));
        lines.push(Line::from(""));
    }

    // Search bar / count header
    if state.search_active || !state.search.is_empty() {
        let cursor = if state.search_active { "▌" } else { "" };
        lines.push(Line::from(Span::styled(
            fit(&format!("  / {}{}", state.search, cursor), w),
            Style::default().fg(palette::ACCENT),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "  {} of {} match{}",
                vis.len(),
                state.entries.len(),
                if vis.len() == 1 { "" } else { "es" }
            ),
            Style::default().fg(palette::TEXT_DIM),
        )));
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "  {} extension{} available",
                state.entries.len(),
                if state.entries.len() == 1 { "" } else { "s" }
            ),
            Style::default().fg(palette::TEXT_DIM),
        )));
        lines.push(Line::from(""));
    }

    if vis.is_empty() {
        lines.push(Line::from(Span::styled(
            fit("  nothing matches — try a different search", w),
            Style::default().fg(palette::TEXT_DIM),
        )));
        frame.render_widget(Paragraph::new(lines), padded);
        return;
    }

    // Split visible indices into official and community buckets, preserving order.
    let official_vis: Vec<usize> = vis.iter().copied()
        .filter(|&i| state.official_ids.contains(&state.entries[i].id))
        .collect();
    let community_vis: Vec<usize> = vis.iter().copied()
        .filter(|&i| !state.official_ids.contains(&state.entries[i].id))
        .collect();

    // Build a flat render order with section headers injected.
    // Each item is (entries_real_idx, vis_pos_in_original_vis).
    let mut render_order: Vec<(usize, usize)> = Vec::new();
    let has_both = !official_vis.is_empty() && !community_vis.is_empty();

    if !official_vis.is_empty() {
        if has_both {
            lines.push(Line::from(Span::styled(
                fit("  official", w),
                Style::default().fg(palette::HIGHLIGHT).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }
        for real_idx in &official_vis {
            let vis_pos = vis.iter().position(|&i| i == *real_idx).unwrap_or(0);
            render_order.push((*real_idx, vis_pos));
        }
    }

    if has_both {
        // Emit official entries first, then separator before community.
        for (real_idx, vis_pos) in &render_order {
            emit_entry(state, *real_idx, *vis_pos, w, &mut lines);
        }
        render_order.clear();
        lines.push(Line::from(Span::styled(
            "  ".to_string() + &"─".repeat(w.saturating_sub(2)),
            Style::default().fg(palette::BORDER),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            fit("  community", w),
            Style::default().fg(palette::TEXT_DIM).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for real_idx in &community_vis {
            let vis_pos = vis.iter().position(|&i| i == *real_idx).unwrap_or(0);
            emit_entry(state, *real_idx, vis_pos, w, &mut lines);
        }
    } else {
        // Only one section — no headers needed.
        for real_idx in official_vis.iter().chain(community_vis.iter()) {
            let vis_pos = vis.iter().position(|&i| i == *real_idx).unwrap_or(0);
            emit_entry(state, *real_idx, vis_pos, w, &mut lines);
        }
    }

    frame.render_widget(Paragraph::new(lines), padded);
}

// ── Detail view ───────────────────────────────────────────────────────────────

fn render_detail(state: &ExtBrowserState, frame: &mut Frame, area: Rect) {
    let Some(real_idx) = state.selected_real_idx() else { return };
    let entry  = &state.entries[real_idx];
    let padded = area.inner(Margin { horizontal: 2, vertical: 1 });

    let total      = state.sub_page_count();
    let dots_area  = Rect { height: 1, ..padded };
    let content_area = Rect {
        y:      padded.y + 2,
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

    let official = state.official_ids.contains(&entry.id);
    let w = padded.width as usize;
    let lines: Vec<Line<'_>> = match state.sub_page {
        0 => detail_overview_inner(entry, w, official),
        1 => detail_commands(entry, w),
        _ => vec![],
    };
    frame.render_widget(Paragraph::new(lines), content_area);
}

fn detail_overview_inner(entry: &RegistryEntry, w: usize, official: bool) -> Vec<Line<'_>> {
    const LABEL_W: usize = 14;
    let val_w = w.saturating_sub(LABEL_W);
    let lbl = |k: &'static str| Span::styled(format!("{k:<LABEL_W$}"), Style::default().fg(palette::TEXT_DIM));
    let val = |v: String| Span::styled(v, Style::default().fg(palette::TEXT));

    let version  = format!("v{}", entry.version);
    let name_max = w.saturating_sub(2 + version.len());
    let name     = fit(&entry.name, name_max);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(name, Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(version, Style::default().fg(palette::TEXT_DIM)),
        ]),
        Line::from(""),
        Line::from(Span::styled(fit(&entry.description, w), Style::default().fg(palette::TEXT_DIM))),
        Line::from(""),
        Line::from(Span::styled("─".repeat(w), Style::default().fg(palette::BORDER))),
        Line::from(""),
        Line::from(vec![lbl("command"), val(fit(&format!("!{}", entry.command_alias()), val_w))]),
        Line::from(vec![lbl("status"),  val(fit(&entry.status, val_w))]),
    ];

    if official {
        lines.push(Line::from(vec![
            lbl("origin"),
            Span::styled("✦ official", Style::default().fg(palette::HIGHLIGHT)),
        ]));
    }

    if let Some(url) = &entry.payload_url {
        if !url.is_empty() {
            lines.push(Line::from(vec![lbl("source"), val(fit(url, val_w))]));
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

    const USAGE_W: usize = 22;
    const INDENT: usize  = 2;
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
            Span::styled(fit(&cmd.description, desc_w), Style::default().fg(palette::TEXT_DIM)),
        ]));
    }

    lines
}

// ── Nav bar ───────────────────────────────────────────────────────────────────

fn nav_line(state: &ExtBrowserState) -> Line<'_> {
    let dim    = Style::default().fg(palette::TEXT_DIM);
    let on_clr = Style::default().fg(palette::ACCENT);

    if state.in_detail {
        let total = state.sub_page_count();
        let mut spans = vec![Span::styled("[←][→] pages  ", dim)];
        if total > 1 {
            for i in 0..total {
                if i > 0 { spans.push(Span::raw("  ")); }
                if i == state.sub_page {
                    spans.push(Span::styled("●", on_clr));
                } else {
                    spans.push(Span::styled("○", dim));
                }
            }
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled("[B] back  [Q] close", dim));
        Line::from(spans)
    } else if state.search_active {
        Line::from(vec![
            Span::styled("[↑][↓] nav  ", dim),
            Span::styled("type to filter", on_clr),
            Span::styled("  [Esc] clear  [Q] close", dim),
        ])
    } else {
        let real = state.selected_real_idx();
        let on        = real.map(|i| state.enabled.contains(&state.entries[i].id)).unwrap_or(false);
        let installed = real.and_then(|i| state.installed.get(i).copied()).unwrap_or(false);

        let toggle_label = if on { "[Spc] disable" } else { "[Spc] enable" };

        let mut spans: Vec<Span<'_>> = vec![
            Span::styled("[↑][↓]  [↵] detail  ", dim),
            Span::styled(toggle_label, on_clr),
        ];
        if !installed { spans.push(Span::styled("  [I] install", dim)); }
        if  installed { spans.push(Span::styled("  [U] remove",  dim)); }
        spans.push(Span::styled("  [/] search  [Q] close", dim));
        Line::from(spans)
    }
}
