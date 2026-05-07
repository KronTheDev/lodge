//! ratatui renderer for the interactive report screen.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::palette::{ACCENT, BORDER, BG, CANDLE, DIM, ERROR, SUCCESS, SURFACE, TEXT, WARN};
use crate::report::selector::{FlaggedFile, SelectionState};
use crate::report::tiers::Tier;

/// Format a byte count as a human-readable size string.
fn fmt_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".into();
    }
    let mb = bytes / (1024 * 1024);
    if mb >= 1 {
        format!("{mb} MB")
    } else {
        let kb = bytes / 1024;
        if kb >= 1 {
            format!("{kb} KB")
        } else {
            format!("{bytes} B")
        }
    }
}

/// Shorten a path to fit within `max_len` characters, truncating the middle.
fn shorten_path(path: &std::path::Path, max_len: usize) -> String {
    let s = path.to_string_lossy();
    if s.len() <= max_len {
        return s.to_string();
    }
    let half = (max_len / 2).saturating_sub(2);
    let start = &s[..half];
    let end = &s[s.len() - half..];
    format!("{start}...{end}")
}

/// Render the full interactive report screen.
pub fn render(state: &SelectionState, no_ai: bool, frame: &mut Frame) {
    let area = frame.area();

    // Background fill
    frame.render_widget(
        Block::default().style(Style::default().bg(BG)),
        area,
    );

    // Outer layout: header | body | footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if no_ai { 4 } else { 3 }),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(state, no_ai, frame, chunks[0]);
    render_body(state, frame, chunks[1]);
    render_footer(state, frame, chunks[2]);
}

fn render_header(state: &SelectionState, no_ai: bool, frame: &mut Frame, area: Rect) {
    let total_files: usize = state.files.len();
    let recoverable = fmt_size(
        state.files.iter().map(|f| f.entry.size).sum(),
    );

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  clean cabin", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("  ·  ", Style::default().fg(DIM)),
            Span::styled(
                format!("{total_files} files flagged  ·  {recoverable} recoverable"),
                Style::default().fg(DIM),
            ),
        ]),
    ];

    if no_ai {
        lines.push(Line::from(Span::styled(
            "  running without AI — you decide tier may be inaccurate",
            Style::default().fg(WARN),
        )));
    }

    lines.push(Line::from(Span::styled(
        "  ─".repeat((area.width as usize / 2).max(1)),
        Style::default().fg(BORDER),
    )));

    let para = Paragraph::new(lines).style(Style::default().bg(SURFACE));
    frame.render_widget(para, area);
}

fn render_body(state: &SelectionState, frame: &mut Frame, area: Rect) {
    let visible = state.visible_files();
    let width = area.width as usize;

    let mut lines: Vec<Line> = Vec::new();

    // ── ClearOut section ──────────────────────────────────────────────────────
    {
        let count = state.tier_count(Tier::ClearOut);
        let size = fmt_size(state.tier_size(Tier::ClearOut));
        let selected_all = state
            .files
            .iter()
            .filter(|f| f.tier == Tier::ClearOut && !f.guarded)
            .all(|f| f.selected);
        let badge = if selected_all { "✔ all" } else { "○" };

        lines.push(Line::from(vec![
            Span::styled("  clear out", Style::default().fg(ERROR).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("  ({count} files · {size})"),
                Style::default().fg(DIM),
            ),
            Span::styled(
                format!("  [{badge}]"),
                Style::default().fg(ACCENT),
            ),
        ]));
    }

    for (idx, file) in visible.iter().filter(|(_, f)| f.tier == Tier::ClearOut) {
        lines.push(render_file_line(*idx, file, state.cursor, width));
    }

    lines.push(divider_line(width));

    // ── WorthALook section ────────────────────────────────────────────────────
    {
        let count = state.tier_count(Tier::WorthALook);
        let size = fmt_size(state.tier_size(Tier::WorthALook));

        lines.push(Line::from(vec![
            Span::styled(
                "  worth a look",
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({count} files · {size})"),
                Style::default().fg(DIM),
            ),
        ]));
    }

    for (idx, file) in visible.iter().filter(|(_, f)| f.tier == Tier::WorthALook) {
        lines.push(render_file_line(*idx, file, state.cursor, width));
    }

    lines.push(divider_line(width));

    // ── YouDecide section ─────────────────────────────────────────────────────
    {
        let count = state.tier_count(Tier::YouDecide);
        let expand_hint = if state.you_decide_expanded {
            "→ collapse"
        } else {
            "→ expand"
        };

        lines.push(Line::from(vec![
            Span::styled(
                "  you decide",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  ({count} files)"), Style::default().fg(DIM)),
            Span::styled(format!("  [{expand_hint}]"), Style::default().fg(DIM)),
        ]));

        if state.you_decide_expanded {
            for (idx, file) in visible.iter().filter(|(_, f)| f.tier == Tier::YouDecide) {
                lines.push(render_file_line(*idx, file, state.cursor, width));
            }
        }
    }

    let para = Paragraph::new(lines)
        .style(Style::default().bg(BG))
        .block(Block::default().borders(Borders::NONE));

    frame.render_widget(para, area);
}

fn render_file_line(
    _file_idx: usize,
    file: &FlaggedFile,
    cursor: usize,
    width: usize,
) -> Line<'static> {
    // Determine visual cursor — check against visible_files order
    let is_focused = false; // will be handled by caller passing cursor position
    let _ = (cursor, is_focused);

    let checkbox = if file.selected {
        Span::styled("  ✔  ", Style::default().fg(ACCENT))
    } else {
        Span::styled("  ○  ", Style::default().fg(DIM))
    };

    let name = file
        .entry
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shorten_path(&file.entry.path, 40));

    let size_str = fmt_size(file.entry.size);

    let reason_suffix = if file.guarded {
        format!(
            "{}  lodge installed this — are you sure?",
            if file.reason.is_empty() { String::new() } else { format!("{}  ", file.reason) }
        )
    } else {
        file.reason.clone()
    };

    // Truncate name + reason to fit width.
    let name_col = 30.min(width / 3);
    let size_col = 8;
    let reason_max = width.saturating_sub(name_col + size_col + 10);
    let reason_display: String = reason_suffix.chars().take(reason_max).collect();

    let name_display: String = if name.len() > name_col {
        format!("{}…", &name[..name_col.saturating_sub(1)])
    } else {
        format!("{name:<name_col$}")
    };

    let guard_span = if file.guarded {
        Span::styled("!  ", Style::default().fg(ERROR))
    } else {
        Span::raw("   ")
    };

    Line::from(vec![
        checkbox,
        guard_span,
        Span::styled(name_display, Style::default().fg(TEXT)),
        Span::raw("  "),
        Span::styled(format!("{size_str:<size_col$}"), Style::default().fg(DIM)),
        Span::styled(reason_display, Style::default().fg(DIM)),
    ])
}

fn divider_line(width: usize) -> Line<'static> {
    let line: String = "─".repeat(width.saturating_sub(2));
    Line::from(Span::styled(format!("  {line}"), Style::default().fg(BORDER)))
}

fn render_footer(state: &SelectionState, frame: &mut Frame, area: Rect) {
    let n = state.selected_files().len();
    let size = fmt_size(state.selected_size());

    let lines = vec![
        Line::from(Span::styled(
            format!("  selected: {n} files · {size}"),
            Style::default().fg(CANDLE),
        )),
        Line::from(Span::styled(
            "  [Space] toggle  [A] all  [N] none  [Tab] tier  [→] expand  [Enter] proceed  [Q] quit",
            Style::default().fg(DIM),
        )),
    ];

    let para = Paragraph::new(lines)
        .style(Style::default().bg(SURFACE))
        .alignment(Alignment::Left);

    frame.render_widget(para, area);
}

/// Render the confirmation screen before staging.
pub fn render_confirmation(
    n: usize,
    size: u64,
    session_id: &str,
    retention_days: u32,
    staging_root: &std::path::Path,
    frame: &mut Frame,
) {
    let area = frame.area();

    frame.render_widget(
        Block::default().style(Style::default().bg(BG)),
        area,
    );

    let size_str = fmt_size(size);
    let staged_path = staging_root.join(session_id);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  moving {n} files to staging."),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  nothing is permanently deleted yet.",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  they'll be held for {retention_days} days in:"),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            format!("  {}", staged_path.display()),
            Style::default().fg(ACCENT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  recover anytime with:  clean-cabin recover",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  total size: {size_str}"),
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  [Enter] move to staging   [Esc] go back",
            Style::default().fg(CANDLE),
        )),
    ];

    let para = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(para, area);
}

/// Render a single staging progress line.
#[allow(dead_code)]
pub fn render_staging_line(name: &str, done: bool, active: bool, frame: &mut Frame, y: u16) {
    let area = Rect {
        x: 0,
        y,
        width: frame.area().width,
        height: 1,
    };

    let (icon, icon_style) = if done {
        ("✔", Style::default().fg(SUCCESS))
    } else if active {
        ("◐", Style::default().fg(crate::palette::FROST))
    } else {
        ("·", Style::default().fg(DIM))
    };

    let line = Line::from(vec![
        Span::styled(format!("  {icon}  "), icon_style),
        Span::styled(name.to_string(), Style::default().fg(TEXT)),
        Span::styled("  → staging", Style::default().fg(DIM)),
    ]);

    frame.render_widget(Paragraph::new(vec![line]), area);
}
