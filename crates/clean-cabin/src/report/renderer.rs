//! ratatui renderer for the interactive report screen.

use std::path::Path;
use std::time::SystemTime;

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::palette::{ACCENT, BORDER, BG, CANDLE, DIM, ERROR, FROST, SUCCESS, SURFACE, TEXT, WARN};
use crate::report::selector::{FlaggedFile, SelectionState};
use crate::report::tiers::Tier;

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Format a byte count as a human-readable size string.
pub fn fmt_size(bytes: u64) -> String {
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
fn shorten_path(path: &Path, max_len: usize) -> String {
    let s = path.to_string_lossy();
    if s.len() <= max_len {
        return s.to_string();
    }
    let half = (max_len / 2).saturating_sub(2);
    let start = &s[..half];
    let end = &s[s.len() - half..];
    format!("{start}...{end}")
}

/// Truncate a string to `max` chars, appending `…` if truncated.
fn fit(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

/// Format a `SystemTime` as a date string `YYYY-MM-DD`, or `"unknown"`.
fn fmt_time(t: Option<SystemTime>) -> String {
    let t = match t {
        Some(t) => t,
        None => return "unknown".into(),
    };
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs() as i64;
            // Very simple date calculation: good enough for display.
            let days_since_epoch = secs / 86400;
            // Use chrono if available; fall back to raw display.
            let naive = chrono::DateTime::from_timestamp(secs, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| format!("epoch+{}d", days_since_epoch));
            naive
        }
        Err(_) => "unknown".into(),
    }
}

/// Compute a centred `Rect` of `width` × `height` inside `area`.
fn centred_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

// ── Main report render ────────────────────────────────────────────────────────

/// Render the full interactive report screen.
pub fn render(state: &SelectionState, no_ai: bool, dirs: &[std::path::PathBuf], frame: &mut Frame) {
    let area = frame.area();

    // Background fill.
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    // Outer layout: header | body | footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if no_ai { 4 } else { 3 }),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(state, no_ai, dirs, frame, chunks[0]);
    render_body(state, frame, chunks[1]);
    render_footer(state, frame, chunks[2]);

    // Detail panel overlay — rendered on top of everything else.
    if let Some(visible_idx) = state.detail_file {
        let visible = state.visible_files();
        if let Some((_, file)) = visible.get(visible_idx) {
            render_detail_panel(file, frame);
        }
    }
}

fn render_header(
    state: &SelectionState,
    no_ai: bool,
    dirs: &[std::path::PathBuf],
    frame: &mut Frame,
    area: Rect,
) {
    let total_files: usize = state.files.len();
    let recoverable = fmt_size(state.files.iter().map(|f| f.entry.size).sum());

    // Build a compact dirs label.
    let dirs_label = if dirs.is_empty() {
        String::new()
    } else if dirs.len() == 1 {
        dirs[0].to_string_lossy().to_string()
    } else {
        format!("{} directories", dirs.len())
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(
            "  clean cabin",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(DIM)),
        Span::styled(fit(&dirs_label, 40), Style::default().fg(DIM)),
        Span::styled("  ·  ", Style::default().fg(DIM)),
        Span::styled(
            format!("{total_files} files flagged  ·  {recoverable} recoverable"),
            Style::default().fg(DIM),
        ),
    ])];

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
        let non_guarded_count = state
            .files
            .iter()
            .filter(|f| f.tier == Tier::ClearOut && !f.guarded)
            .count();
        let selected_all = non_guarded_count > 0
            && state
                .files
                .iter()
                .filter(|f| f.tier == Tier::ClearOut && !f.guarded)
                .all(|f| f.selected);
        let badge = if selected_all { "all: ✔" } else { "all: ○" };

        lines.push(Line::from(vec![
            Span::styled(
                "  clear out",
                Style::default().fg(ERROR).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({count} files · {size})"),
                Style::default().fg(DIM),
            ),
            Span::styled(format!("  [{badge}]"), Style::default().fg(ACCENT)),
        ]));
    }

    // Enumerate visible files to get correct visible_idx for cursor comparison.
    let mut visible_idx = 0usize;
    for (_, file) in visible.iter().filter(|(_, f)| f.tier == Tier::ClearOut) {
        lines.push(render_file_line(visible_idx, file, state.cursor, width));
        visible_idx += 1;
    }

    lines.push(divider_line(width));

    // ── WorthALook section ────────────────────────────────────────────────────
    {
        let count = state.tier_count(Tier::WorthALook);
        let size = fmt_size(state.tier_size(Tier::WorthALook));
        let badge = "all: ○";

        lines.push(Line::from(vec![
            Span::styled(
                "  worth a look",
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({count} files · {size})"),
                Style::default().fg(DIM),
            ),
            Span::styled(format!("  [{badge}]"), Style::default().fg(DIM)),
        ]));
    }

    for (_, file) in visible.iter().filter(|(_, f)| f.tier == Tier::WorthALook) {
        lines.push(render_file_line(visible_idx, file, state.cursor, width));
        visible_idx += 1;
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
            for (_, file) in visible.iter().filter(|(_, f)| f.tier == Tier::YouDecide) {
                lines.push(render_file_line(visible_idx, file, state.cursor, width));
                visible_idx += 1;
            }
        }
    }

    let para = Paragraph::new(lines)
        .style(Style::default().bg(BG))
        .block(Block::default().borders(Borders::NONE));

    frame.render_widget(para, area);
}

fn render_file_line(
    visible_idx: usize,
    file: &FlaggedFile,
    cursor: usize,
    width: usize,
) -> Line<'static> {
    let is_focused = visible_idx == cursor;

    let row_style = if is_focused {
        Style::default().bg(CANDLE).fg(BG)
    } else {
        Style::default()
    };

    let checkbox = if file.selected {
        Span::styled(
            "  ✔  ",
            if is_focused {
                Style::default().fg(BG).bg(CANDLE)
            } else {
                Style::default().fg(ACCENT)
            },
        )
    } else {
        Span::styled(
            "  ○  ",
            if is_focused {
                Style::default().fg(BG).bg(CANDLE)
            } else {
                Style::default().fg(DIM)
            },
        )
    };

    let name = file
        .entry
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shorten_path(&file.entry.path, 40));

    let size_str = fmt_size(file.entry.size);

    let reason_suffix = if file.guarded {
        if file.reason.is_empty() {
            "lodge installed this — are you sure?".to_string()
        } else {
            format!("{}  lodge installed this — are you sure?", file.reason)
        }
    } else {
        file.reason.clone()
    };

    let name_col = 30.min(width / 3);
    let size_col = 8;
    let reason_max = width.saturating_sub(name_col + size_col + 12);
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

    let text_style = if is_focused {
        Style::default().fg(BG).bg(CANDLE)
    } else {
        Style::default().fg(TEXT)
    };
    let dim_style = if is_focused {
        Style::default().fg(BG).bg(CANDLE)
    } else {
        Style::default().fg(DIM)
    };

    Line::from(vec![
        checkbox,
        Span::styled("   ", row_style),
        guard_span,
        Span::styled(name_display, text_style),
        Span::styled("  ", row_style),
        Span::styled(format!("{size_str:<size_col$}"), dim_style),
        Span::styled(reason_display, dim_style),
    ])
}

fn divider_line(width: usize) -> Line<'static> {
    let line: String = "─".repeat(width.saturating_sub(2));
    Line::from(Span::styled(
        format!("  {line}"),
        Style::default().fg(BORDER),
    ))
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
            "  [Space] toggle  [A] all  [N] none  [Tab] tier  [→] expand  [Enter] detail/proceed  [Q] quit",
            Style::default().fg(DIM),
        )),
    ];

    let para = Paragraph::new(lines)
        .style(Style::default().bg(SURFACE))
        .alignment(Alignment::Left);

    frame.render_widget(para, area);
}

/// Render a centered detail panel overlay for a flagged file.
pub fn render_detail_panel(file: &FlaggedFile, frame: &mut Frame) {
    let area = frame.area();

    let card_width = (area.width.saturating_sub(8)).min(72);
    let card_height = 16u16;
    let rect = centred_rect(card_width, card_height, area);

    frame.render_widget(Clear, rect);

    let full_path = file.entry.path.to_string_lossy();
    let size_str = fmt_size(file.entry.size);
    let modified = fmt_time(file.entry.modified);
    let accessed = fmt_time(file.entry.accessed);

    let inner_w = (card_width.saturating_sub(4)) as usize;

    let tier_label = match file.tier {
        Tier::ClearOut => "clear out",
        Tier::WorthALook => "worth a look",
        Tier::YouDecide => "you decide",
    };
    let tier_color = match file.tier {
        Tier::ClearOut => ERROR,
        Tier::WorthALook => WARN,
        Tier::YouDecide => TEXT,
    };

    let guard_line = if file.guarded {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                fit("  lodge installed this — confirm before including.", inner_w),
                Style::default().fg(ERROR),
            )),
        ]
    } else {
        vec![]
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                fit(
                    &file
                        .entry
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| full_path.to_string()),
                    inner_w,
                ),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  path      ", Style::default().fg(DIM)),
            Span::styled(fit(&full_path, inner_w.saturating_sub(12)), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  size      ", Style::default().fg(DIM)),
            Span::styled(size_str, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  modified  ", Style::default().fg(DIM)),
            Span::styled(modified, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  accessed  ", Style::default().fg(DIM)),
            Span::styled(accessed, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  tier      ", Style::default().fg(DIM)),
            Span::styled(tier_label, Style::default().fg(tier_color)),
        ]),
        Line::from(vec![
            Span::styled("  reason    ", Style::default().fg(DIM)),
            Span::styled(
                fit(&file.reason, inner_w.saturating_sub(12)),
                Style::default().fg(TEXT),
            ),
        ]),
    ];

    lines.extend(guard_line);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  [Esc] close",
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(""));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(SURFACE));

    let para = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(SURFACE));

    frame.render_widget(para, rect);
}

/// Render the confirmation card (centered overlay, not fullscreen).
pub fn render_confirmation(
    n: usize,
    size: u64,
    session_id: &str,
    retention_days: u32,
    staging_root: &Path,
    frame: &mut Frame,
) {
    let area = frame.area();

    // Dim background.
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let card_width = (area.width.saturating_sub(8)).min(68);
    let card_height = 14u16;
    let rect = centred_rect(card_width, card_height, area);

    frame.render_widget(Clear, rect);

    let size_str = fmt_size(size);
    let staged_path = staging_root.join(session_id);
    let inner_w = (card_width.saturating_sub(4)) as usize;

    let expiry = crate::cabin_trash::staging::expiry_date(session_id, retention_days)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".into());

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            fit(&format!("  moving {n} files to staging."), inner_w),
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
            fit(&format!("  {}", staged_path.display()), inner_w),
            Style::default().fg(ACCENT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  recover anytime with:  !clean recover",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            fit(
                &format!("  {n} files · {size_str} · auto-purge {expiry}"),
                inner_w,
            ),
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  [Enter] move to staging   [Esc] go back",
            Style::default().fg(CANDLE),
        )),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(SURFACE));

    let para = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(SURFACE));

    frame.render_widget(para, rect);
}

// ── Scan progress ─────────────────────────────────────────────────────────────

/// Render the scan progress screen.
///
/// `dirs_label` is a short string describing what is being scanned.
/// `file_count` is the current live count. `flagged` is how many were flagged
/// so far. `spinner_frame` cycles 0..3 to animate `◐◑◒◓`.
pub fn render_scan_progress(
    dirs_label: &str,
    file_count: usize,
    flagged: usize,
    spinner_frame: u8,
    frame: &mut Frame,
) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let spinner = match spinner_frame % 4 {
        0 => "◐",
        1 => "◑",
        2 => "◒",
        _ => "◓",
    };

    // Gauge: use file_count / 10000 as proxy for progress (unknown total).
    let ratio = (file_count as f64 / 10000.0_f64).min(1.0);
    let gauge_width = (area.width.saturating_sub(4)) as usize;
    let filled = (ratio * gauge_width as f64) as usize;
    let empty = gauge_width.saturating_sub(filled);
    let gauge_str = format!("  {}{}", "▓".repeat(filled), "░".repeat(empty));

    let label = format!(
        "  {spinner}  {file_count} files examined · {flagged} flagged so far..."
    );

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  clean cabin", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("  ·  scanning ", Style::default().fg(DIM)),
            Span::styled(dirs_label.to_string(), Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(Span::styled(gauge_str, Style::default().fg(FROST))),
        Line::from(""),
        Line::from(Span::styled(label, Style::default().fg(DIM))),
    ];

    let para = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(para, area);
}

// ── Directory prompt ──────────────────────────────────────────────────────────

/// Render the directory prompt card.
///
/// `confirmed_dirs` is the list of already-confirmed directories.
/// `exclusions` is the list of configured exclusions.
/// `input` is the current text in the input field.
/// `exclusion_mode` is true when the user is entering exclusion patterns.
pub fn render_dir_prompt(
    confirmed_dirs: &[std::path::PathBuf],
    exclusions: &[String],
    input: &str,
    exclusion_mode: bool,
    frame: &mut Frame,
) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let card_width = (area.width.saturating_sub(8)).min(60);
    let inner_w = (card_width.saturating_sub(4)) as usize;

    // Calculate card height dynamically.
    let dir_lines = confirmed_dirs.len();
    let excl_lines = if exclusions.is_empty() { 0 } else { exclusions.len() + 1 };
    let card_height = (10 + dir_lines + excl_lines) as u16;

    let rect = centred_rect(card_width, card_height.max(12), area);
    frame.render_widget(Clear, rect);

    let prompt_label = if exclusion_mode {
        "  add exclusion pattern:"
    } else {
        "  which directories should we look through?"
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  clean cabin",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            prompt_label,
            Style::default().fg(TEXT),
        )),
        Line::from(vec![
            Span::styled("  > ", Style::default().fg(ACCENT)),
            Span::styled(input.to_string(), Style::default().fg(TEXT)),
            Span::styled("_", Style::default().fg(CANDLE)),
        ]),
        Line::from(""),
    ];

    // Show confirmed dirs.
    if !confirmed_dirs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  directories:",
            Style::default().fg(DIM),
        )));
        for d in confirmed_dirs {
            lines.push(Line::from(Span::styled(
                fit(&format!("    + {}", d.display()), inner_w),
                Style::default().fg(SUCCESS),
            )));
        }
        lines.push(Line::from(""));
    }

    // Show exclusions.
    if !exclusions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  exclusions:",
            Style::default().fg(DIM),
        )));
        for ex in exclusions {
            lines.push(Line::from(Span::styled(
                fit(&format!("    - {ex}"), inner_w),
                Style::default().fg(WARN),
            )));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "  [Enter] confirm   [Tab] add another",
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "  [E] add exclusion   [Esc] cancel",
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(""));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(SURFACE));

    let para = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(SURFACE));

    frame.render_widget(para, rect);
}

// ── Staging sequence ──────────────────────────────────────────────────────────

/// Render the live staging sequence.
///
/// `files` is the full manifest file list.
/// `done_count` is how many have been moved.
/// `active_idx` is the index currently being processed.
pub fn render_staging_sequence(
    files: &[crate::cabin_trash::staging::StagedFile],
    done_count: usize,
    active_idx: Option<usize>,
    frame: &mut Frame,
) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  clean cabin  ·  moving to staging",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ─────────────────────────────────────────────────────",
            Style::default().fg(BORDER),
        )),
        Line::from(""),
    ];

    for (i, sf) in files.iter().enumerate() {
        let name = sf
            .original_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| sf.staged_name.clone());
        let name_display = fit(&name, 30);

        let (icon, icon_style, suffix_style) = if i < done_count {
            (
                "✔",
                Style::default().fg(SUCCESS),
                Style::default().fg(DIM),
            )
        } else if active_idx == Some(i) {
            (
                "◐",
                Style::default().fg(FROST),
                Style::default().fg(DIM),
            )
        } else {
            (
                "·",
                Style::default().fg(DIM),
                Style::default().fg(DIM),
            )
        };

        let suffix = if i < done_count {
            "  → staging"
        } else if active_idx == Some(i) {
            "  → staging..."
        } else {
            ""
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {icon}  "), icon_style),
            Span::styled(format!("{name_display:<30}"), Style::default().fg(TEXT)),
            Span::styled(suffix, suffix_style),
        ]));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(para, area);
}

/// Render the staging completion summary.
pub fn render_staging_complete(
    n: usize,
    total_size: u64,
    expiry: &str,
    frame: &mut Frame,
) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let size_str = fmt_size(total_size);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  clean cabin  ·  done",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ─────────────────────────────────────────────────────",
            Style::default().fg(BORDER),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {n} files moved to staging. {size_str} freed."),
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            format!("  they'll be permanently removed on {expiry} unless recovered."),
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  recover anytime with:  !clean recover",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  press any key to continue",
            Style::default().fg(CANDLE),
        )),
    ];

    let para = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(para, area);
}

// ── Recovery session picker ───────────────────────────────────────────────────

/// Render the recovery session picker.
pub fn render_recover_picker(
    sessions: &[(String, crate::cabin_trash::staging::StagingManifest)],
    cursor: usize,
    frame: &mut Frame,
) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  clean cabin  ·  recover",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ─────────────────────────────────────────────────────",
            Style::default().fg(BORDER),
        )),
        Line::from(""),
    ];

    for (i, (session_id, manifest)) in sessions.iter().enumerate() {
        let total_size: u64 = manifest.files.iter().map(|f| f.size).sum();
        let size_str = fmt_size(total_size);
        let label = format!(
            "  {}  {}  ·  {} files · {}",
            if i == cursor { "▶" } else { " " },
            session_id,
            manifest.files.len(),
            size_str,
        );

        let style = if i == cursor {
            Style::default().fg(CANDLE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT)
        };

        lines.push(Line::from(Span::styled(label, style)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  [↑/↓] navigate  [Enter] restore  [Esc] cancel",
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(""));

    let para = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(para, area);
}

/// Render the recover progress / completion screen.
pub fn render_recover_result(session_id: &str, restored: usize, total: usize, frame: &mut Frame) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  clean cabin  ·  recovered",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ─────────────────────────────────────────────────────",
            Style::default().fg(BORDER),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  session {session_id}"),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            format!("  {restored} / {total} files restored."),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  press any key to continue",
            Style::default().fg(CANDLE),
        )),
    ];

    let para = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(para, area);
}

// ── Purge confirmation card ───────────────────────────────────────────────────

/// Render the purge confirmation card.
pub fn render_purge_confirm(session_count: usize, frame: &mut Frame) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let card_width = (area.width.saturating_sub(8)).min(60);
    let card_height = 12u16;
    let rect = centred_rect(card_width, card_height, area);

    frame.render_widget(Clear, rect);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  purge all staged sessions?",
            Style::default().fg(ERROR).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {session_count} sessions will be permanently deleted."),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "  this cannot be undone.",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  files staged with !clean will be gone for good.",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  [Enter] purge all   [Esc] cancel",
            Style::default().fg(CANDLE),
        )),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(ERROR))
        .style(Style::default().bg(SURFACE));

    let para = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(SURFACE));

    frame.render_widget(para, rect);
}

/// Render purge completion summary.
pub fn render_purge_done(session_count: usize, total_freed: u64, frame: &mut Frame) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let size_str = fmt_size(total_freed);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  clean cabin  ·  purged",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  ─────────────────────────────────────────────────────",
            Style::default().fg(BORDER),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {session_count} sessions purged. {size_str} freed."),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  press any key to continue",
            Style::default().fg(CANDLE),
        )),
    ];

    let para = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(para, area);
}
