// First-startup onboarding sequence.
//
// Nine animated screens introduce Lodge, its extensions, and the opt-in
// Claude API integration. Text lines reveal progressively (~150 ms apart).
// Pressing Space/Enter skips the animation then advances. S skips to done.

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::Backend,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame, Terminal,
};

use super::palette;

// ── Screen indices ────────────────────────────────────────────────────────────

const WELCOME: usize = 0;
const HOW_IT_WORKS: usize = 1;
const COMMAND_BAR: usize = 2;
const LODGER: usize = 3;
const EXTENSIONS: usize = 4; // interactive selection — no line animation
const EXTENDING: usize = 5;
const AI: usize = 6;
const AI_PROVIDER: usize = 7; // provider picker + optional key input
const DONE: usize = 8;
const TOTAL: usize = 9;

// (AI_OPT_* constants removed — provider list is now driven by lodge_brain::ai::PROVIDERS)

// Ticks between each line appearing (50 ms poll × 3 = 150 ms)
const TICKS_PER_LINE: usize = 3;
// Extra ticks after full reveal before the nav hint appears
const NAV_DELAY: usize = 5;

// ── Extension manifest (read from extensions/*.json) ──────────────────────────

#[derive(Debug, Clone)]
struct ExtManifest {
    id: String,
    name: String,
    version: String,
    description: String,
    /// "stable" | "preview" | "coming-soon"
    status: String,
    /// zip filename inside the extensions/ dir, if a payload exists
    payload: Option<String>,
}

#[derive(Debug, Clone)]
struct ExtEntry {
    manifest: ExtManifest,
    /// payload zip is already present on disk (ships with Lodge)
    local_ready: bool,
    /// payload is obtainable: either local_ready or a download URL exists
    available: bool,
    /// user may toggle (false for coming-soon)
    selectable: bool,
    selected: bool,
}

fn load_local_extensions() -> Vec<ExtEntry> {
    let dir = crate::engine::extensions::extensions_dir();
    let mut entries: Vec<ExtEntry> = Vec::new();

    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return entries;
    };

    for item in read_dir.flatten() {
        let path = item.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };

        let manifest = ExtManifest {
            id: val["id"].as_str().unwrap_or("").to_string(),
            name: val["name"].as_str().unwrap_or("unknown").to_string(),
            version: val["version"].as_str().unwrap_or("").to_string(),
            description: val["description"].as_str().unwrap_or("").to_string(),
            status: val["status"].as_str().unwrap_or("preview").to_string(),
            payload: val["payload"].as_str().map(str::to_string),
        };

        if manifest.id.is_empty() {
            continue;
        }

        let local_ready = manifest
            .payload
            .as_ref()
            .map(|p| dir.join(p).exists())
            .unwrap_or(false);
        let available = local_ready;

        let selectable = manifest.status != "coming-soon";
        // Auto-select stable or preview if the payload is present
        let selected = selectable && available;

        entries.push(ExtEntry { manifest, local_ready, available, selectable, selected });
    }

    // Sort: stable first, preview second, coming-soon last
    entries.sort_by_key(|e| match e.manifest.status.as_str() {
        "stable" => 0u8,
        "preview" => 1,
        _ => 2,
    });

    entries
}

fn load_extensions_from_registry(entries: &[crate::engine::extensions::RegistryEntry]) -> Vec<ExtEntry> {
    let dir = crate::engine::extensions::extensions_dir();
    let mut result: Vec<ExtEntry> = Vec::new();

    for reg in entries {
        // Available if the payload zip is already on disk (ships with Lodge)
        // OR if a remote payload_url is present (can be downloaded).
        let local_ready = reg.payload.as_ref()
            .map(|p| dir.join(p).exists())
            .unwrap_or(false);
        let downloadable = reg.payload_url.as_deref()
            .map(|u| !u.is_empty())
            .unwrap_or(false);
        let available = local_ready || downloadable;
        let selectable = reg.status != "coming-soon";
        let selected = selectable && available;

        result.push(ExtEntry {
            manifest: ExtManifest {
                id: reg.id.clone(),
                name: reg.name.clone(),
                version: reg.version.clone(),
                description: reg.description.clone(),
                status: reg.status.clone(),
                payload: reg.payload.clone(),
            },
            local_ready,
            available,
            selectable,
            selected,
        });
    }

    result.sort_by_key(|e| match e.manifest.status.as_str() {
        "stable" => 0u8,
        "preview" => 1,
        _ => 2,
    });
    result
}

// ── State ─────────────────────────────────────────────────────────────────────

struct State {
    screen: usize,
    tick: usize,
    /// loaded extension list with selection flags
    extensions: Vec<ExtEntry>,
    /// true if the extension registry was fetched from GitHub this session
    extensions_online: bool,
    /// receiver for the background registry fetch result
    ext_fetch_rx: Option<std::sync::mpsc::Receiver<(Vec<crate::engine::extensions::RegistryEntry>, bool)>>,
    /// cursor row in the EXTENSIONS screen
    ext_cursor: usize,
    /// cursor in the AI_PROVIDER picker (index into lodge_brain::ai::PROVIDERS)
    ai_cursor: usize,
    /// scroll offset for the AI provider picker
    ai_scroll: usize,
    /// true when the key entry sub-mode is active on the AI_PROVIDER screen
    ai_key_mode: bool,
    /// true when the custom-provider model-string entry sub-mode is active
    ai_model_mode: bool,
    /// key typed so far in AI_PROVIDER key entry
    ai_key_buf: String,
    /// model string typed in the custom-provider model entry sub-mode
    ai_model_buf: String,
    /// result of the Ollama reachability check: None=not yet checked
    ai_ollama_ok: Option<bool>,
    /// set once a provider has been saved
    ai_provider_saved: bool,
}

impl State {
    fn new() -> Self {
        // Kick off registry fetch in background
        let (ext_tx, ext_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::engine::extensions::fetch_registry();
            let _ = ext_tx.send(result);
        });

        // Start with local extensions dir as fallback while fetch is in flight
        let local_entries = load_local_extensions();

        State {
            screen: WELCOME,
            tick: 0,
            extensions: local_entries,
            extensions_online: false,
            ext_fetch_rx: Some(ext_rx),
            ext_cursor: 0,
            ai_cursor: 1, // default to Ollama (index 1 in PROVIDERS)
            ai_scroll: 0,
            ai_key_mode: false,
            ai_model_mode: false,
            ai_key_buf: String::new(),
            ai_model_buf: String::new(),
            ai_ollama_ok: None,
            ai_provider_saved: false,
        }
    }

    fn visible_lines(&self) -> usize {
        self.tick / TICKS_PER_LINE
    }

    fn all_revealed(&self, total: usize) -> bool {
        self.visible_lines() >= total
    }

    fn nav_ready(&self, total: usize) -> bool {
        self.tick >= total * TICKS_PER_LINE + NAV_DELAY
    }

    fn reveal_all(&mut self, total: usize) {
        let needed = total * TICKS_PER_LINE + NAV_DELAY + 1;
        if self.tick < needed {
            self.tick = needed;
        }
    }

    fn advance(&mut self) {
        self.screen += 1;
        self.tick = 0;
    }
}

// ── First-run marker ──────────────────────────────────────────────────────────

/// `true` when Lodge has never been run before on this machine.
pub fn is_first_run() -> bool {
    !marker_path().map(|p| p.exists()).unwrap_or(false)
}

fn marker_path() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let base = std::env::var("LOCALAPPDATA").ok()?;
    #[cfg(not(windows))]
    let base = std::env::var("HOME")
        .ok()
        .map(|h| format!("{h}/.local/share"))?;
    Some(
        std::path::PathBuf::from(base)
            .join("lodge")
            .join(".onboarding_done"),
    )
}

fn mark_done() {
    if let Some(path) = marker_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, "");
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Runs the full onboarding sequence, then marks first-run complete.
pub fn run<B: Backend>(terminal: &mut Terminal<B>) -> anyhow::Result<()> {
    let mut state = State::new();

    loop {
        // Poll the background extension fetch
        if let Some(rx) = &state.ext_fetch_rx {
            if let Ok((entries, online)) = rx.try_recv() {
                state.extensions = load_extensions_from_registry(&entries);
                state.extensions_online = online;
                state.ext_fetch_rx = None;
            }
        }

        terminal.draw(|f| render(&state, f))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if handle_key(&mut state, key.code) {
                    break;
                }
            }
        } else {
            state.tick += 1;
        }

        if state.screen >= TOTAL {
            break;
        }
    }

    mark_done();
    Ok(())
}

/// Returns `true` when the sequence is complete.
fn handle_key(state: &mut State, code: KeyCode) -> bool {
    // S skips to the next screen from any screen except AI_PROVIDER and EXTENSIONS
    if matches!(code, KeyCode::Char('s') | KeyCode::Char('S'))
        && !matches!(state.screen, AI_PROVIDER | EXTENSIONS)
    {
        state.advance();
        return false;
    }

    match state.screen {
        // ── EXTENSIONS — interactive multi-select ────────────────────────────
        EXTENSIONS => match code {
            KeyCode::Up if state.ext_cursor > 0 => {
                state.ext_cursor -= 1;
            }
            KeyCode::Down if state.ext_cursor + 1 < state.extensions.len() => {
                state.ext_cursor += 1;
            }
            KeyCode::Char(' ') => {
                if let Some(e) = state.extensions.get_mut(state.ext_cursor) {
                    if e.selectable {
                        e.selected = !e.selected;
                    }
                }
            }
            KeyCode::Enter => state.advance(),
            KeyCode::Esc => state.advance(), // skip without changes
            _ => {}
        },

        // ── AI_PROVIDER — provider picker + optional key input ───────────────
        AI_PROVIDER => {
            let provider_count = lodge_brain::ai::PROVIDERS.len();

            if state.ai_model_mode {
                // Custom provider: model string entry
                match code {
                    KeyCode::Enter if !state.ai_model_buf.trim().is_empty() => {
                        state.ai_model_mode = false;
                        state.ai_key_mode = true;
                    }
                    KeyCode::Esc => {
                        state.ai_model_mode = false;
                        state.ai_model_buf.clear();
                    }
                    KeyCode::Backspace => { state.ai_model_buf.pop(); }
                    KeyCode::Char(c) => state.ai_model_buf.push(c),
                    _ => {}
                }
            } else if state.ai_key_mode {
                match code {
                    KeyCode::Enter => {
                        let key = state.ai_key_buf.trim().to_string();
                        let provider = &lodge_brain::ai::PROVIDERS[state.ai_cursor];
                        let model = if provider.id == "custom" {
                            state.ai_model_buf.trim().to_string()
                        } else {
                            provider.default_model.to_string()
                        };
                        if !model.is_empty() {
                            state.ai_provider_saved = lodge_brain::ai::save_model_key(
                                provider.id, &model, &key,
                            ).is_ok();
                        }
                        state.advance();
                    }
                    KeyCode::Esc => {
                        state.ai_key_mode = false;
                        state.ai_key_buf.clear();
                    }
                    KeyCode::Backspace => { state.ai_key_buf.pop(); }
                    KeyCode::Char(c) => state.ai_key_buf.push(c),
                    _ => {}
                }
            } else {
                match code {
                    KeyCode::Up if state.ai_cursor > 0 => {
                        state.ai_cursor -= 1;
                        if state.ai_cursor < state.ai_scroll {
                            state.ai_scroll = state.ai_cursor;
                        }
                    }
                    KeyCode::Down if state.ai_cursor + 1 < provider_count => {
                        state.ai_cursor += 1;
                        if state.ai_cursor >= state.ai_scroll + 4 {
                            state.ai_scroll = state.ai_cursor.saturating_sub(3);
                        }
                    }
                    KeyCode::Enter => {
                        let provider = &lodge_brain::ai::PROVIDERS[state.ai_cursor];
                        match provider.id {
                            "none" => {
                                let _ = lodge_brain::ai::clear_config();
                                state.advance();
                            }
                            "ollama" => {
                                let ok = lodge_brain::ai::ollama_reachable();
                                state.ai_ollama_ok = Some(ok);
                                if ok {
                                    let _ = lodge_brain::ai::save_model_key(
                                        "ollama", lodge_brain::ai::MODEL_OLLAMA, "",
                                    );
                                    state.ai_provider_saved = true;
                                }
                                state.advance();
                            }
                            "custom" => {
                                state.ai_model_mode = true;
                            }
                            _ => {
                                state.ai_key_mode = true;
                            }
                        }
                    }
                    KeyCode::Esc => state.advance(),
                    _ => {}
                }
            }
        }

        // ── Done ─────────────────────────────────────────────────────────────
        DONE => {
            if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) {
                return true;
            }
        }

        // ── All other text screens — Space / Enter ────────────────────────────
        _ => {
            if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) {
                let total = screen_line_count(state.screen);
                if !state.nav_ready(total) {
                    state.reveal_all(total);
                } else {
                    state.advance();
                }
            }
        }
    }

    false
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render(state: &State, frame: &mut Frame) {
    let area = frame.area();

    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(palette::BG)),
        area,
    );

    let card_w = 72u16.min(area.width.saturating_sub(4));
    let card_h = 30u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(card_w)) / 2;
    let y = area.y + (area.height.saturating_sub(card_h)) / 2;
    let card_area = Rect { x, y, width: card_w, height: card_h };

    frame.render_widget(Clear, card_area);

    let title = screen_title(state.screen);
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(title, Style::default().fg(palette::ACCENT)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(palette::BORDER))
        .style(Style::default().bg(palette::SURFACE));

    let inner = block.inner(card_area);
    frame.render_widget(block, card_area);

    match state.screen {
        HOW_IT_WORKS => render_how_it_works(state, inner, frame),
        EXTENSIONS   => render_extensions(state, inner, frame),
        AI_PROVIDER  => render_ai_provider(state, inner, frame),
        DONE         => render_done(state, inner, frame),
        _            => render_text_screen(state, inner, frame),
    }
}

// ── Screen metadata ───────────────────────────────────────────────────────────

fn screen_title(screen: usize) -> &'static str {
    match screen {
        WELCOME      => "welcome to lodge.",
        HOW_IT_WORKS => "how it works.",
        COMMAND_BAR  => "the command bar.",
        LODGER       => "lodger.",
        EXTENSIONS   => "choose your extensions.",
        EXTENDING    => "extending lodge.",
        AI           => "ai in lodge.",
        AI_PROVIDER  => "choose an ai provider.",
        DONE         => "you're set.",
        _            => "lodge",
    }
}

/// Number of logical lines that drive animation timing.
/// EXTENSIONS, API_KEY, and DONE are handled separately (not animated line-by-line).
fn screen_line_count(screen: usize) -> usize {
    match screen {
        WELCOME      => 10,
        HOW_IT_WORKS => 13,
        COMMAND_BAR  => 16,
        LODGER       => 11,
        EXTENDING    => 10,
        AI           => 14,
        DONE         => 9,
        _            => 0,
    }
}

// ── Navigation helpers ────────────────────────────────────────────────────────

fn nav_hint(screen: usize) -> Line<'static> {
    let text = if screen == DONE {
        "  [Enter] open the cabin"
    } else {
        "  [Space] continue    [S] skip"
    };
    Line::from(Span::styled(text, Style::default().fg(palette::TEXT_DIM)))
}

fn sep(width: u16) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(width as usize),
        Style::default().fg(palette::BORDER),
    ))
}

// ── Generic text screen ───────────────────────────────────────────────────────

fn render_text_screen(state: &State, area: Rect, frame: &mut Frame) {
    let all = screen_content(state.screen, area.width);
    let total = screen_line_count(state.screen);
    let visible = state.visible_lines().min(all.len());

    let mut lines: Vec<Line> = all.into_iter().take(visible).collect();

    if state.nav_ready(total) {
        lines.push(Line::from(""));
        lines.push(sep(area.width));
        lines.push(nav_hint(state.screen));
    } else if state.all_revealed(total) {
        let dot = if (state.tick / 4).is_multiple_of(2) { "·" } else { " " };
        lines.push(Line::from(Span::styled(
            format!("  {dot}"),
            Style::default().fg(palette::TEXT_DIM),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

// ── HOW IT WORKS — file settling animation ────────────────────────────────────

fn render_how_it_works(state: &State, area: Rect, frame: &mut Frame) {
    let total = screen_line_count(HOW_IT_WORKS);
    let vis = state.visible_lines();
    let mut lines: Vec<Line> = Vec::new();

    let intro: &[&str] = &[
        "",
        "a package ships a manifest describing what it is.",
        "lodge reads it, resolves the right paths for your OS,",
        "and places each file where it belongs.",
        "",
    ];
    for (i, text) in intro.iter().enumerate() {
        if vis > i {
            lines.push(txt(text));
        }
    }

    let settle_base = intro.len();
    let file_rows: &[(&str, &str)] = &[
        ("mytool.exe   ", " AppData\\Local\\Programs\\mytool\\"),
        ("config.json  ", " AppData\\Roaming\\mytool\\"),
        ("mt.cmd       ", " lodge\\shims\\"),
    ];

    for (i, (name, dest)) in file_rows.iter().enumerate() {
        let trigger = settle_base + i * 2;
        if vis > trigger {
            let row_start = trigger * TICKS_PER_LINE;
            let row_tick = state.tick.saturating_sub(row_start);
            let dash_count = (row_tick / 2).min(6);
            let settled = dash_count >= 6;

            let mut spans: Vec<Span> = vec![
                Span::raw("  "),
                Span::styled(*name, Style::default().fg(palette::TEXT)),
            ];
            if dash_count > 0 {
                let dashes = "─".repeat(dash_count);
                let tip = if settled { "►" } else { "─" };
                spans.push(Span::styled(
                    format!("{dashes}{tip}"),
                    Style::default().fg(palette::ACCENT),
                ));
            }
            if settled {
                spans.push(Span::styled(*dest, Style::default().fg(palette::TEXT_DIM)));
            }
            lines.push(Line::from(spans));
        }
    }

    let outro_base = settle_base + file_rows.len() * 2;
    let outro: &[&str] = &["", "every install writes a signed receipt.", "uninstall is always clean."];
    for (i, text) in outro.iter().enumerate() {
        if vis > outro_base + i {
            lines.push(txt(text));
        }
    }

    if state.nav_ready(total) {
        lines.push(Line::from(""));
        lines.push(sep(area.width));
        lines.push(nav_hint(HOW_IT_WORKS));
    } else if state.all_revealed(total) {
        let dot = if (state.tick / 4).is_multiple_of(2) { "·" } else { " " };
        lines.push(Line::from(Span::styled(
            format!("  {dot}"),
            Style::default().fg(palette::TEXT_DIM),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

// ── EXTENSIONS — interactive multi-select ────────────────────────────────────

fn render_extensions(state: &State, area: Rect, frame: &mut Frame) {
    let mut lines: Vec<Line> = vec![
        Line::from(""),
        txt("select what to install alongside Lodge."),
        dim("you can change this later with  `!ext`"),
        Line::from(""),
        sep(area.width),
        Line::from(""),
    ];

    // Show fetch status indicator
    if state.ext_fetch_rx.is_some() {
        lines.push(Line::from(Span::styled(
            "  · fetching extension list...",
            Style::default().fg(palette::TEXT_DIM),
        )));
        lines.push(Line::from(""));
    } else if !state.extensions_online {
        lines.push(Line::from(Span::styled(
            "  ! extension list may be out of date — couldn't reach github",
            Style::default().fg(palette::WARNING),
        )));
        lines.push(Line::from(""));
    }

    if state.extensions.is_empty() {
        lines.push(dim("no extensions found in the extensions/ directory."));
        lines.push(dim("drop extension packages there and rerun lodge to pick them up."));
    } else {
        for (i, entry) in state.extensions.iter().enumerate() {
            let cursor = if i == state.ext_cursor { "▶" } else { " " };
            let check = if entry.selected { "✓" } else { " " };

            // Status badge: label + colour
            let (badge_label, badge_style) = match entry.manifest.status.as_str() {
                "stable"       => (entry.manifest.version.as_str(), Style::default().fg(palette::SUCCESS)),
                "preview"      => (entry.manifest.version.as_str(), Style::default().fg(palette::WARNING)),
                "coming-soon"  => ("coming soon",                   Style::default().fg(palette::TEXT_DIM)),
                _              => (entry.manifest.version.as_str(), Style::default().fg(palette::TEXT_DIM)),
            };

            // First line: cursor + checkbox + name + badge
            // Prefix: "  {cursor} [{check}] " = 9 chars; suffix: "  {badge}" = 2 + badge len.
            let check_style = if entry.selectable {
                Style::default().fg(palette::ACCENT)
            } else {
                Style::default().fg(palette::TEXT_DIM)
            };

            let name_style = if entry.selectable {
                Style::default().fg(palette::TEXT)
            } else {
                Style::default().fg(palette::TEXT_DIM)
            };

            let badge_str   = badge_label.to_string();
            let prefix_w    = 9usize; // "  {cursor} [{check}] "
            let suffix_w    = 2 + badge_str.len(); // "  {badge}"
            let name_max    = (area.width as usize).saturating_sub(prefix_w + suffix_w);
            let name_fitted = {
                let chars: Vec<char> = entry.manifest.name.chars().collect();
                if chars.len() <= name_max {
                    entry.manifest.name.clone()
                } else {
                    chars[..name_max.saturating_sub(1)].iter().collect::<String>() + "…"
                }
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {cursor} "), Style::default().fg(palette::ACCENT)),
                Span::styled(format!("[{check}] "), check_style),
                Span::styled(name_fitted, name_style),
                Span::raw("  "),
                Span::styled(badge_str, badge_style),
            ]));

            // Second line: description — indent 7 spaces, clip to panel width.
            let desc_style = if entry.selectable {
                Style::default().fg(palette::TEXT_DIM)
            } else {
                Style::default().fg(palette::BORDER)
            };
            let desc_indent = 7usize;
            let desc_max    = (area.width as usize).saturating_sub(desc_indent);
            let desc_fitted = {
                let chars: Vec<char> = entry.manifest.description.chars().collect();
                if chars.len() <= desc_max {
                    entry.manifest.description.clone()
                } else {
                    chars[..desc_max.saturating_sub(1)].iter().collect::<String>() + "…"
                }
            };
            lines.push(Line::from(Span::styled(
                format!("       {desc_fitted}"),
                desc_style,
            )));

            // Third line: availability note (three states)
            let avail_text = if entry.manifest.status == "coming-soon" {
                "       · not yet available".to_string()
            } else if entry.local_ready {
                "       · payload ready".to_string()
            } else if entry.available {
                // available via payload_url but not yet downloaded
                "       · available for download".to_string()
            } else {
                "       · payload not found — selection noted for when it ships".to_string()
            };
            lines.push(Line::from(Span::styled(
                avail_text,
                Style::default().fg(palette::BORDER),
            )));

            lines.push(Line::from(""));
        }
    }

    lines.push(sep(area.width));
    lines.push(Line::from(vec![
        Span::styled("  [↑↓] navigate   ", Style::default().fg(palette::TEXT_DIM)),
        Span::styled("[Space] toggle   ", Style::default().fg(palette::ACCENT)),
        Span::styled("[Enter] continue", Style::default().fg(palette::TEXT_DIM)),
    ]));

    frame.render_widget(Paragraph::new(lines), area);
}

// ── AI_PROVIDER screen ────────────────────────────────────────────────────────

fn render_ai_provider(state: &State, area: Rect, frame: &mut Frame) {
    if state.ai_model_mode {
        render_ai_model_input(state, area, frame);
        return;
    }
    if state.ai_key_mode {
        render_ai_key_input(state, area, frame);
        return;
    }

    let providers = lodge_brain::ai::PROVIDERS;
    let visible_count = 4usize; // providers visible at once
    let scroll = state.ai_scroll;
    let can_scroll_up = scroll > 0;
    let can_scroll_down = scroll + visible_count < providers.len();

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        txt("lodge can explain probe results and narrate scans."),
        txt("pick an AI provider. any genai-compatible model works."),
        Line::from(""),
        sep(area.width),
        Line::from(""),
    ];

    if can_scroll_up {
        lines.push(Line::from(Span::styled("  ↑ more above", Style::default().fg(palette::TEXT_DIM))));
    }

    for (i, p) in providers
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_count)
    {
        let cursor = if i == state.ai_cursor { "▶" } else { " " };
        let name_style = if i == state.ai_cursor {
            Style::default().fg(palette::ACCENT)
        } else {
            Style::default().fg(palette::TEXT)
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {cursor} "), Style::default().fg(palette::ACCENT)),
            Span::styled(p.name.to_string(), name_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("       {}", p.key_hint),
            Style::default().fg(palette::TEXT_DIM),
        )));

        // Ollama: show reachability result if checked
        if p.id == "ollama" {
            if let Some(ok) = state.ai_ollama_ok {
                let (msg, sty) = if ok {
                    ("       ✔ Ollama found", Style::default().fg(palette::SUCCESS))
                } else {
                    ("       · Ollama not running — install it first, then reopen Lodge",
                     Style::default().fg(palette::WARNING))
                };
                lines.push(Line::from(Span::styled(msg, sty)));
            }
        }

        lines.push(Line::from(""));
    }

    if can_scroll_down {
        lines.push(Line::from(Span::styled("  ↓ more below", Style::default().fg(palette::TEXT_DIM))));
    }

    lines.push(sep(area.width));
    lines.push(Line::from(vec![
        Span::styled("  [↑↓] navigate   ", Style::default().fg(palette::TEXT_DIM)),
        Span::styled("[Enter] select   ", Style::default().fg(palette::ACCENT)),
        Span::styled("[Esc] skip", Style::default().fg(palette::TEXT_DIM)),
    ]));

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_ai_model_input(state: &State, area: Rect, frame: &mut Frame) {
    let lines: Vec<Line> = vec![
        Line::from(""),
        txt("enter the genai model string for your provider."),
        dim("examples: gpt-4o-mini  llama-3.3-70b-versatile  command-r"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  > ", Style::default().fg(palette::ACCENT)),
            Span::styled(state.ai_model_buf.clone(), Style::default().fg(palette::TEXT)),
            Span::styled("_", Style::default().fg(palette::HIGHLIGHT)),
        ]),
        Line::from(""),
        sep(area.width),
        Line::from(vec![
            Span::styled("  [Enter] next    ", Style::default().fg(palette::ACCENT)),
            Span::styled("[Esc] back", Style::default().fg(palette::TEXT_DIM)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_ai_key_input(state: &State, area: Rect, frame: &mut Frame) {
    let provider = &lodge_brain::ai::PROVIDERS[state.ai_cursor];
    let provider_name = provider.name;
    let key_hint = provider.key_hint;

    let display_key: String = state
        .ai_key_buf
        .chars()
        .enumerate()
        .map(|(i, c)| if i < 7 { c } else { '·' })
        .collect();

    let lines: Vec<Line> = vec![
        Line::from(""),
        txt(Box::leak(format!("paste your {provider_name} API key below.").into_boxed_str())),
        dim(Box::leak(key_hint.to_string().into_boxed_str())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  > ", Style::default().fg(palette::ACCENT)),
            Span::styled(display_key, Style::default().fg(palette::TEXT)),
            Span::styled("_", Style::default().fg(palette::HIGHLIGHT)),
        ]),
        Line::from(""),
        sep(area.width),
        Line::from(vec![
            Span::styled("  [Enter] save    ", Style::default().fg(palette::ACCENT)),
            Span::styled("[Esc] back", Style::default().fg(palette::TEXT_DIM)),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), area);
}

// ── DONE screen ───────────────────────────────────────────────────────────────

fn render_done(state: &State, area: Rect, frame: &mut Frame) {
    let total = screen_line_count(DONE);
    let vis = state.visible_lines();

    let all_content: Vec<Line> = if state.ai_provider_saved {
        vec![
            Line::from(""),
            txt("AI provider saved."),
            Line::from(""),
            dim("type  help         to see what's available."),
            dim("type  install <id> to settle in a package."),
            dim("type  scan         to read this machine."),
            dim("type  expand       to go deeper on any probe result."),
            Line::from(""),
            accent("everything has a place. yours is here."),
        ]
    } else {
        vec![
            Line::from(""),
            txt("lodge is ready."),
            Line::from(""),
            dim("type  help         to see what's available."),
            dim("type  install <id> to settle in a package."),
            dim("type  scan         to read this machine."),
            dim("type  lodge help ai  to set up an AI provider later."),
            Line::from(""),
            accent("everything has a place. yours is here."),
        ]
    };

    let mut lines: Vec<Line> = all_content.into_iter().take(vis.min(8)).collect();

    if state.nav_ready(total) {
        lines.push(Line::from(""));
        lines.push(sep(area.width));
        lines.push(nav_hint(DONE));
    } else if state.all_revealed(total) {
        let dot = if (state.tick / 4).is_multiple_of(2) { "·" } else { " " };
        lines.push(Line::from(Span::styled(
            format!("  {dot}"),
            Style::default().fg(palette::TEXT_DIM),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

// ── Screen content (text-only screens) ───────────────────────────────────────

fn screen_content(screen: usize, _width: u16) -> Vec<Line<'static>> {
    match screen {
        WELCOME      => content_welcome(),
        COMMAND_BAR  => content_command_bar(),
        LODGER       => content_lodger(),
        EXTENDING    => content_extending(),
        AI           => content_ai(),
        _            => vec![],
    }
}

fn content_welcome() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        accent("a place for everything."),
        Line::from(""),
        txt("lodge is an installation runtime. it reads a package,"),
        txt("figures out where everything belongs on your machine,"),
        txt("and settles it in."),
        Line::from(""),
        txt("no configuration required. no guesswork. no noise."),
        Line::from(""),
        txt("everything finds its place."),
    ]
}

fn content_command_bar() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        txt("the command bar understands you. type naturally."),
        Line::from(""),
        cmd("install mytool",   "settle a package in"),
        cmd("uninstall mytool", "clean removal"),
        cmd("update mytool",    "update to latest"),
        cmd("list",             "what's installed"),
        cmd("verify mytool",    "check file integrity"),
        cmd("history",          "installation log"),
        Line::from(""),
        txt("or just ask:"),
        Line::from(""),
        q("do I have node installed?"),
        q("how much RAM do I have?"),
        q("is port 8080 free?"),
        Line::from(""),
    ]
}

fn content_lodger() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        txt("lodge ships an optional desktop companion: lodger."),
        Line::from(""),
        txt("lodger is a small pixel-art figure that lives on your"),
        txt("desktop while lodge runs. it reacts to installations,"),
        txt("probes, and failures — and can speak aloud via your"),
        txt("system's text-to-speech API."),
        Line::from(""),
        dim("drop lodger.exe alongside lodge.exe to enable it."),
        dim("toggle voice with  `lodger on`  and  `lodger off`."),
        dim("if lodger.exe is absent, lodge works exactly as normal."),
    ]
}

fn content_extending() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        txt("lodge ships with built-in rules for Windows, macOS,"),
        txt("and Linux. community rulesets add support for new"),
        txt("package types and non-standard conventions."),
        Line::from(""),
        cmd("update rulesets", "pull the latest community rules"),
        Line::from(""),
        txt("more integrations are on the way. each one is opt-in."),
        txt("lodge stays small — you add what you need."),
        Line::from(""),
    ]
}

fn content_ai() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        txt("lodge can narrate probe results and summarise your"),
        txt("machine in plain language. this uses an AI provider."),
        Line::from(""),
        txt("any genai-compatible model is supported:"),
        Line::from(""),
        tier("Ollama",   "local · free · runs on your machine"),
        tier("Gemini",   "free cloud tier · Google AI Studio key"),
        tier("Claude",   "Anthropic API · requires credits"),
        tier("OpenAI",   "GPT-4o mini · platform.openai.com"),
        tier("Groq",     "free tier · very fast inference"),
        tier("xAI",      "Grok models · console.x.ai"),
        tier("DeepSeek", "very cheap · platform.deepseek.com"),
        tier("Cohere",   "free tier · command-r model"),
        Line::from(""),
    ]
}

// ── Line builders ─────────────────────────────────────────────────────────────

fn txt(s: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {s}"),
        Style::default().fg(palette::TEXT),
    ))
}

fn dim(s: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {s}"),
        Style::default().fg(palette::TEXT_DIM),
    ))
}

fn accent(s: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {s}"),
        Style::default().fg(palette::ACCENT),
    ))
}

fn cmd(command: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(command, Style::default().fg(palette::ACCENT)),
        Span::styled(
            format!("{:width$}{desc}", "", width = 22usize.saturating_sub(command.len())),
            Style::default().fg(palette::TEXT_DIM),
        ),
    ])
}

fn q(question: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {question}"),
        Style::default().fg(palette::TEXT),
    ))
}

fn tier(name: &'static str, note: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(name, Style::default().fg(palette::ACCENT)),
        Span::styled(
            format!("{:width$}{note}", "", width = 14usize.saturating_sub(name.len())),
            Style::default().fg(palette::TEXT_DIM),
        ),
    ])
}
