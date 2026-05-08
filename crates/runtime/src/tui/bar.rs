use std::sync::{Arc, Mutex};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Terminal,
};

use lodge_brain::{Brain, Command};

use super::{ext_browser, flashcard, help, onboarding, palette, splash};
use crate::engine::attester;

// ── Input mode types ──────────────────────────────────────────────────────────

/// Visual / behavioural category of a recognised input.
#[derive(Clone, Copy, PartialEq, Default)]
enum CmdKind {
    #[default]
    None,       // natural language, explore, clarify
    Command,    // structural: !help, !list, !scan …  (gets ! prefix)
    Directive,  // executive: install, uninstall, rollback … (no ! prefix)
}

/// Animates synonym → canonical in the input row after submit (e.g. "get" → "install").
///
/// Phase 1 — replace synonym chars with ░ left-to-right.
/// Phase 2 — write canonical chars left-to-right over the ░ field.
/// The `suffix` (everything after the synonym, e.g. " python") is kept static.
struct SynonymAnim {
    synonym:   Vec<char>,
    canonical: Vec<char>,
    suffix:    String,
    progress:  usize,
    last_tick: std::time::Instant,
}

impl SynonymAnim {
    /// ms per animation step.  10 steps for "get"→"install" = 200 ms total.
    const STEP_MS: u64 = 20;

    fn new(synonym: &str, canonical: &str, suffix: &str) -> Self {
        Self {
            synonym:   synonym.chars().collect(),
            canonical: canonical.chars().collect(),
            suffix:    suffix.to_string(),
            progress:  0,
            last_tick: std::time::Instant::now(),
        }
    }

    /// Advance one step if enough time has passed.  Returns `true` when done.
    fn tick(&mut self) -> bool {
        let total = self.synonym.len() + self.canonical.len();
        if self.progress < total
            && self.last_tick.elapsed()
                >= std::time::Duration::from_millis(Self::STEP_MS)
        {
            self.progress += 1;
            self.last_tick = std::time::Instant::now();
        }
        self.progress >= total
    }

    /// Full text to display in the input row (animated verb + static suffix).
    fn display(&self) -> String {
        const FILL: char = '░';
        let syn_len = self.synonym.len();
        let can_len = self.canonical.len();
        let max_len = syn_len.max(can_len);

        let verb: String = if self.progress <= syn_len {
            // Phase 1: erase synonym with ░, pad right to canonical length
            let mut chars: Vec<char> = self.synonym.clone();
            for c in chars.iter_mut().take(self.progress) {
                *c = FILL;
            }
            while chars.len() < max_len {
                chars.push(FILL);
            }
            chars.into_iter().collect()
        } else {
            // Phase 2: write canonical, fill remainder with ░
            let written = self.progress - syn_len;
            let mut s: Vec<char> = self.canonical[..written].to_vec();
            s.extend(std::iter::repeat_n(FILL, can_len - written));
            s.into_iter().collect()
        };
        format!("{verb}{}", self.suffix)
    }
}

/// Wipe-up animation played when `!clr` is submitted.
///
/// One entry is removed from the bottom every STEP_MS milliseconds so the
/// sweep is perceptible regardless of how many entries are in history.
/// Two-phase right-to-left clear animation, driven entirely by elapsed time.
///
/// Phase 1 (progress 0.0 → 1.0): every history line's characters are replaced
/// by ░ blocks simultaneously, sweeping from the rightmost column leftward.
///
/// Phase 2 (progress 1.0 → 2.0): the ░ blocks are deleted the same way,
/// right-to-left, until the area is blank — then `terminal.clear()` fires.
///
/// Total wall-clock duration: 2 × PHASE_MS = 2 500 ms.
struct ClrAnim {
    /// Original history — passed straight to render_bar; the mask is applied
    /// there after the lines are built, so wrapping / prefixes are correct.
    snapshot: Vec<(String, String)>,
    start:    std::time::Instant,
}

impl ClrAnim {
    /// Duration of each phase (░ fill and ░ wipe) in milliseconds.
    const PHASE_MS: u64 = 750;

    fn new(snapshot: Vec<(String, String)>) -> Self {
        Self { snapshot, start: std::time::Instant::now() }
    }

    /// Normalised progress: 0.0 = just started · 1.0 = phase 1 done · 2.0 = complete.
    fn progress(&self) -> f32 {
        let elapsed = self.start.elapsed().as_millis() as f32;
        (elapsed / Self::PHASE_MS as f32).min(2.0)
    }

    /// Returns `true` when both phases are done and `terminal.clear()` should fire.
    fn tick(&self) -> bool {
        self.progress() >= 2.0
    }

    /// The original history slice — passed to `render_bar` as `hist_display`.
    fn snapshot(&self) -> &[(String, String)] {
        &self.snapshot
    }
}

/// A live stat slot in the right-hand status band.
/// One live display slot in the right-hand status band.
struct Runspace {
    title:           String,
    value:           String,          // raw probe value text
    typed:           usize,           // chars revealed so far (typewriter)
    last_typed:      std::time::Instant,
    /// Named probe + its args, enables live refresh every REFRESH_SECS seconds.
    live_probe:      Option<(String, std::collections::HashMap<String, String>)>,
    last_refresh:    std::time::Instant,
    /// Spinner frame (0–3 → ◐◑◒◓), advances whether refreshing or idle on live slots.
    spin_frame:      u8,
    last_spin_tick:  std::time::Instant,
    /// Fill level of the progress bar, animated from 0.0 toward target_pct/100.
    bar_fill:        f32,
    /// Parsed usage percentage target (None = no bar shown).
    target_pct:      Option<f32>,
}

impl Runspace {
    const TYPE_MS:      u64   = 6;    // ms per typewriter tick
    const TYPE_SPEED:   usize = 3;    // chars per tick
    const SPIN_MS:      u64   = 130;  // ms per spinner step
    const REFRESH_SECS: u64   = 12;
    const BAR_STEP:     f32   = 0.025; // fill increment per 16ms frame

    fn new(title: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        let target_pct = parse_usage_pct(&value);
        Self {
            title:          title.into(),
            value,
            typed:          0,
            last_typed:     std::time::Instant::now(),
            live_probe:     None,
            last_refresh:   std::time::Instant::now(),
            spin_frame:     0,
            last_spin_tick: std::time::Instant::now(),
            bar_fill:       0.0,
            target_pct,
        }
    }

    fn with_live(mut self, probe: String, args: std::collections::HashMap<String, String>) -> Self {
        self.live_probe = Some((probe, args));
        self
    }

    /// Install a fresh value (e.g. after live refresh). Restarts animations.
    fn set_value(&mut self, value: String) {
        self.value    = value;
        self.typed    = 0;
        self.bar_fill = 0.0;
        self.target_pct = parse_usage_pct(&self.value);
    }

    fn tick(&mut self) {
        // Typewriter
        let total = self.value.len();
        if self.typed < total
            && self.last_typed.elapsed() >= std::time::Duration::from_millis(Self::TYPE_MS)
        {
            self.typed = (self.typed + Self::TYPE_SPEED).min(total);
            self.last_typed = std::time::Instant::now();
        }
        // Spinner (always ticks on live slots for the "system is alive" feel)
        if self.live_probe.is_some()
            && self.last_spin_tick.elapsed() >= std::time::Duration::from_millis(Self::SPIN_MS)
        {
            self.spin_frame = (self.spin_frame + 1) % 4;
            self.last_spin_tick = std::time::Instant::now();
        }
        // Animated bar fill
        if let Some(target) = self.target_pct {
            let target_fill = (target / 100.0).clamp(0.0, 1.0);
            if self.bar_fill < target_fill {
                self.bar_fill = (self.bar_fill + Self::BAR_STEP).min(target_fill);
            } else {
                self.bar_fill = target_fill;
            }
        }
    }

    fn is_animating(&self) -> bool {
        self.typed < self.value.len()
    }

    fn needs_fast_tick(&self) -> bool {
        self.is_animating()
            || self.target_pct
                .map(|t| self.bar_fill < (t / 100.0) - 0.001)
                .unwrap_or(false)
    }

    fn display(&self) -> &str {
        let end = self.typed.min(self.value.len());
        let mut e = end;
        while e > 0 && !self.value.is_char_boundary(e) { e -= 1; }
        &self.value[..e]
    }
}

/// Pre-compute which suggestion (if any) is under the mouse.
///
/// Uses the same geometry as render_bar: header=1, divider=1, history=Min(0),
/// mid_divider=1, input=input_rows, bottom_divider=1.
fn compute_hover_suggestion(
    hover_row: Option<u16>,
    history: &[(String, String)],
    history_scroll: u16,
    split_mode: bool,
    term_height: u16,
) -> Option<String> {
    let hy = hover_row?;
    let input_rows = if split_mode { 2u16 } else { 1u16 };
    let hist_y: u16 = 2;
    let hist_h: u16 = term_height.saturating_sub(4 + input_rows);
    if hist_h == 0 { return None; }

    let flat = build_flat_history_text(history);
    let hist_len = flat.len() as u16;

    let line_idx: usize = if hist_len <= hist_h {
        let start = (hist_y + hist_h).saturating_sub(hist_len);
        if hy < start || hy >= hist_y + hist_h { return None; }
        (hy - start) as usize
    } else {
        let base = hist_len - hist_h;
        let offset = base.saturating_sub(history_scroll);
        if hy < hist_y || hy >= hist_y + hist_h { return None; }
        (hy - hist_y + offset) as usize
    };

    flat.get(line_idx).and_then(|line| {
        line.trim_start()
            .strip_prefix('✦')
            .map(|rest| rest.trim().to_string())
    })
}

// ── Band helpers ──────────────────────────────────────────────────────────────

/// Extract up to three floating-point numbers from a string in order.
fn extract_floats(s: &str) -> Vec<f32> {
    let mut nums: Vec<f32> = Vec::new();
    let mut in_num = false;
    let mut start = 0usize;
    let mut has_dot = false;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || (c == '.' && in_num && !has_dot) {
            if !in_num { start = i; in_num = true; }
            if c == '.' { has_dot = true; }
        } else if in_num {
            if let Ok(n) = s[start..i].parse::<f32>() { nums.push(n); }
            in_num = false;
            has_dot = false;
            if nums.len() >= 3 { break; }
        }
    }
    if in_num {
        if let Ok(n) = s[start..].parse::<f32>() { nums.push(n); }
    }
    nums
}

/// Parse a "used / total" or "free / total" ratio from a probe value string.
/// Returns the *used* percentage (0–100).
fn parse_usage_pct(value: &str) -> Option<f32> {
    let nums = extract_floats(value);
    if nums.len() < 2 || nums[1] < 0.001 { return None; }
    let lower = value.to_lowercase();
    // Detect "X free (of Y)" or "X free / Y" patterns → used = (Y - X) / Y
    let free_idx  = lower.find("free");
    let of_idx    = lower.find("(of").or_else(|| lower.find(" of "));
    let slash_idx = lower.find('/');
    let is_free_first = free_idx.is_some_and(|fi| {
        let after = of_idx.or(slash_idx).unwrap_or(usize::MAX);
        fi < after
    });
    if is_free_first {
        Some(((nums[1] - nums[0]) / nums[1] * 100.0).clamp(0.0, 100.0))
    } else {
        Some((nums[0] / nums[1] * 100.0).clamp(0.0, 100.0))
    }
}

/// Spinner frames for live runspaces.
const SPIN: [char; 4] = ['◐', '◑', '◒', '◓'];

/// Pick a bar/pct color by usage level.
fn usage_color(pct: f32) -> ratatui::style::Color {
    if pct >= 85.0      { palette::ERROR   }
    else if pct >= 65.0 { palette::WARNING }
    else                { palette::SUCCESS }
}

/// Render the right-hand status band.
///
/// Each runspace shows:
///   · Title bar with live spinner (◐◑◒◓) when probe is live
///   · Typewriter-revealed value text (with blinking cursor ▌)
///   · Animated fill progress bar + percentage when a ratio is parseable
fn render_band(
    runspaces: &[Runspace],
    area:      ratatui::layout::Rect,
    frame:     &mut ratatui::Frame,
) {
    use ratatui::layout::Rect;
    if area.width < 4 { return; }

    let w     = area.width as usize;
    let inner = w.saturating_sub(1); // 1-char left pad

    let mut y = area.y;

    for (ri, rs) in runspaces.iter().enumerate() {
        if y >= area.y + area.height { break; }

        // ── Title bar ──────────────────────────────────────────────────────────
        // Format: " title ── ◑ ──────" (live) or " title ────────────" (static)
        let spinner_str = if rs.live_probe.is_some() {
            format!(" {} ", SPIN[rs.spin_frame as usize])
        } else {
            String::new()
        };
        let title_chars: usize = rs.title.chars().count();
        let spin_chars:  usize = spinner_str.chars().count();
        // dashes fill the rest: 1 pad + title + spin + dashes = inner
        let dash_n = inner
            .saturating_sub(title_chars + spin_chars + 1); // +1 for space after title

        let title_line = if rs.live_probe.is_some() {
            Line::from(vec![
                Span::styled(format!(" {}", rs.title), Style::default().fg(palette::ACCENT)),
                Span::styled(" ".to_string(), Style::default().fg(palette::BORDER)),
                Span::styled("─".repeat(dash_n / 2), Style::default().fg(palette::BORDER)),
                Span::styled(spinner_str, Style::default().fg(palette::IN_PROGRESS)),
                Span::styled("─".repeat(dash_n - dash_n / 2), Style::default().fg(palette::BORDER)),
            ])
        } else {
            let dashes = "─".repeat(inner.saturating_sub(title_chars + 1));
            Line::from(vec![
                Span::styled(format!(" {}", rs.title), Style::default().fg(palette::ACCENT)),
                Span::styled(format!(" {dashes}"), Style::default().fg(palette::BORDER)),
            ])
        };
        frame.render_widget(
            Paragraph::new(title_line),
            Rect { x: area.x, y, width: area.width, height: 1 },
        );
        y += 1;

        // ── Value text (typewriter + word-wrap) ───────────────────────────────
        // Wrap long lines so content fills the band rather than being clipped.
        let displayed = rs.display();
        let cursor_sfx = if rs.is_animating() { "▌" } else { "" };
        let full = format!("{displayed}{cursor_sfx}");
        let mut line_count = 0u16;
        'outer: for source_line in full.lines() {
            for wrapped in wrap_at(source_line, inner) {
                if y >= area.y + area.height || line_count >= 6 { break 'outer; }
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!(" {wrapped}"),
                        Style::default().fg(palette::TEXT),
                    ))),
                    Rect { x: area.x, y, width: area.width, height: 1 },
                );
                y += 1;
                line_count += 1;
            }
        }

        // ── Progress bar ───────────────────────────────────────────────────────
        // Only shown when a ratio was parseable and the value has been typed.
        if y < area.y + area.height && rs.target_pct.is_some() && line_count > 0 {
            let pct   = rs.bar_fill * 100.0;
            let color = usage_color(pct);
            // Bar occupies (inner - 5) chars, then " XX%" right-aligned
            let pct_str = format!(" {:>3.0}%", pct);
            let bar_w = inner.saturating_sub(pct_str.len());
            let filled = ((rs.bar_fill * bar_w as f32) as usize).min(bar_w);
            let empty  = bar_w - filled;
            let bar_line = Line::from(vec![
                Span::styled(
                    format!(" {}", "▓".repeat(filled)),
                    Style::default().fg(color),
                ),
                Span::styled(
                    "░".repeat(empty),
                    Style::default().fg(palette::BORDER),
                ),
                Span::styled(pct_str, Style::default().fg(color)),
            ]);
            frame.render_widget(
                Paragraph::new(bar_line),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
            y += 1;
        }

        // ── Separator between runspaces ────────────────────────────────────────
        if ri < runspaces.len() - 1 && y < area.y + area.height {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "─".repeat(w),
                    Style::default().fg(palette::BORDER),
                ))),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
            y += 1;
        }
    }
}

/// Runs the interactive command bar.
///
/// Shows the splash screen, then opens a persistent `> _` prompt.
/// Input is routed through the brain (deterministic resolver + model if loaded).
/// Model inference is dispatched to a background thread so the TUI stays live.
pub fn run() -> anyhow::Result<()> {
    // Generate a short session ID from the low 16 bits of subsecond nanoseconds.
    let session_id = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        format!("{:04x}", nanos & 0xFFFF)
    };
    let window_title = format!("\u{2302} Lodge ({session_id})");

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        // EnableMouseCapture sets ENABLE_MOUSE_INPUT via Win32 on Windows, which is
        // sufficient for MOUSE_MOVED (free cursor movement) events.  The \x1b[?1003h
        // VT sequence was previously added here but it interferes on Windows Terminal:
        // WT injects VT mouse escape sequences into the input buffer while crossterm
        // reads via ReadConsoleInputW, causing mouse movement to arrive as garbled key
        // events.  Removed — Win32 ENABLE_MOUSE_INPUT is all that is needed.
        EnableMouseCapture,
        SetTitle(&window_title)
    )?;

    // Restore terminal on panic
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stderr(), LeaveAlternateScreen);
        original(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Fire-and-forget: refresh the extension registry cache in the background.
    // The cached result is used by the next onboarding or extension command.
    std::thread::spawn(|| { let _ = crate::engine::extensions::fetch_registry(); });

    // Build extension alias list from cached registry for autocomplete.
    // Captures online status for the offline indicator in the ext browser.
    let (all_ext_entries, ext_registry_online) = crate::engine::extensions::fetch_registry();
    let ext_entries: Vec<crate::engine::extensions::RegistryEntry> = all_ext_entries
        .into_iter()
        .filter(|e| e.status != "coming-soon")
        .collect();
    // Fetch official extension IDs (falls back to cache when offline).
    let ext_official_ids = crate::engine::extensions::fetch_official_ids();
    // Load persisted extension enabled/disabled state.
    let mut ext_state = crate::engine::extensions::load_ext_state();
    // Only enabled extensions appear in autocomplete and are dispatchable.
    let mut ext_ids: Vec<String> = ext_entries.iter()
        .filter(|e| ext_state.enabled.contains(&e.id))
        .map(|e| e.command_alias().to_string())
        .collect();

    // Drain any key events that arrived before raw mode was fully active,
    // so they don't accidentally trigger the first screen.
    while event::poll(std::time::Duration::ZERO)? {
        let _ = event::read();
    }

    // First-run onboarding — shown before the splash on a fresh install.
    if onboarding::is_first_run() {
        onboarding::run(&mut terminal)?;
        // Drain again after onboarding so the splash isn't instantly skipped.
        while event::poll(std::time::Duration::ZERO)? {
            let _ = event::read();
        }
    }

    // Show splash screen until a key is pressed (press only — not release)
    loop {
        terminal.draw(splash::render)?;
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    break;
                }
            }
        }
    }

    // Initialise brain (tries to load model, degrades gracefully)
    let brain = Arc::new(Mutex::new(Brain::new()));
    let has_model = brain.lock().unwrap().has_model();

    let model_note = if has_model {
        None
    } else {
        Some("deterministic mode — place smollm2-360m-q4_k_m.gguf alongside the binary to enable AI.")
    };

    let mut input = String::new();
    let mut history: Vec<(String, String)> = Vec::new();
    let mut thinking = false;
    // Help overlay: Some(page) when open, None when closed
    let mut help_page: Option<usize> = None;
    // Extension browser overlay
    let mut ext_overlay: Option<ext_browser::ExtBrowserState> = None;
    // Last probe result — fed to `expand` when the user asks to go deeper
    let mut last_probe: Option<String> = None;
    let mut cursor: usize = 0;             // char-index cursor in `input`
    let mut history_scroll: u16 = 0;      // lines scrolled up from bottom (0 = at bottom)
    // Post-submit synonym animation — renders over last history entry's command text.
    let mut submit_anim: Option<SynonymAnim> = None;
    let mut clr_anim:    Option<ClrAnim>     = None;
    // Split mode — active only for `!command` structural commands.
    let mut split_mode  = false;
    let mut split_cmd   = String::new();  // canonical command (e.g. "verify")
    let mut split_args  = String::new();  // args row content
    let mut split_cursor = 0usize;        // cursor position in args row
    // Command history — navigated when history scroll is at its boundary.
    let mut cmd_history: Vec<String> = Vec::new();
    let mut cmd_history_idx: Option<usize> = None; // None = not navigating (fresh input)
    // Paths logged via `!register` for further processing.
    let mut registered_paths: Vec<String> = Vec::new();
    // When true, registered_paths is cleared after the next submitted prompt.
    let mut register_active = false;
    // Last known mouse row — used to highlight hovered suggestion lines.
    let mut hover_row: Option<u16> = None;
    // Right-hand status band runspaces.
    let mut runspaces: Vec<Runspace> = Vec::new();
    // Tab-navigation index through visible suggestions (None = not navigating).
    let mut tab_suggestion_idx: Option<usize> = None;

    if let Some(note) = model_note {
        history.push((String::new(), note.to_string()));
    }

    // Channel for background inference results: (original_input, response)
    let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();

    loop {
        // Advance post-submit synonym animation.
        if let Some(ref mut a) = submit_anim {
            if a.tick() { submit_anim = None; }
        }

        // Advance clear animation.
        let clr_just_done = if let Some(ref a) = clr_anim {
            if a.tick() { clr_anim = None; true } else { false }
        } else { false };
        // When the animation finishes, directly clear the terminal screen.
        // ratatui's per-cell diff on Windows Terminal can miss cells that
        // were written by a previous overflow-mode Paragraph; terminal.clear()
        // outputs \x1b[2J unconditionally and resets ratatui's buffer so the
        // next draw is a guaranteed full redraw.
        if clr_just_done {
            terminal.clear()?;
        }

        // Tick runspace animations and refresh live probes.
        let needs_fast = runspaces.iter().any(|rs| rs.needs_fast_tick());
        for rs in &mut runspaces {
            rs.tick();
            // Live probe refresh (only when typewriter/bar are done)
            if let Some((probe_name, probe_args)) = rs.live_probe.clone() {
                if rs.last_refresh.elapsed().as_secs() >= Runspace::REFRESH_SECS
                    && !rs.needs_fast_tick()
                {
                    if let Some(result) = lodge_brain::scout::dispatch(&probe_name, &probe_args) {
                        let new_val = result.value.or(result.raw).unwrap_or_default();
                        if new_val != rs.value {
                            rs.set_value(new_val);
                        }
                    }
                    rs.last_refresh = std::time::Instant::now();
                }
            }
        }

        // Compute the maximum scroll offset so Up/Down boundary detection works.
        // This mirrors render_bar's overflow check using the current terminal size.
        let max_history_scroll: u16 = {
            let size = terminal.size().unwrap_or_default();
            let fixed = 5u16 + if split_mode { 1 } else { 0 };
            let avail = size.height.saturating_sub(fixed);
            let total: u16 = history.iter().map(|(cmd, resp)| {
                let cl: u16 = if cmd.is_empty() { 0 } else { 1 };
                let rl: u16 = if resp.is_empty() { 0 } else {
                    resp.split('\n')
                        .map(|p| wrap_at(p, 98).len() as u16)
                        .sum::<u16>()
                        + 1 // blank separator line
                };
                cl + rl
            }).sum();
            total.saturating_sub(avail)
        };
        // Clamp scroll to valid range (content may have shrunk after a redraw).
        history_scroll = history_scroll.min(max_history_scroll);

        // Pre-compute hover suggestion before draw (decouples geometry from render).
        let term_size = terminal.size().unwrap_or_default();
        let hist_for_hover: &[(String, String)] = clr_anim
            .as_ref()
            .map(|a| a.snapshot())
            .unwrap_or(&history);
        let hover_suggestion = compute_hover_suggestion(
            hover_row, hist_for_hover, history_scroll, split_mode, term_size.height,
        );

        // Draw first — guarantees at least one "thinking..." frame is visible
        // before the result can replace it.
        let hist_display: &[(String, String)] = clr_anim
            .as_ref()
            .map(|a| a.snapshot())
            .unwrap_or(&history);
        let clr_progress: Option<f32> = clr_anim.as_ref().map(|a| a.progress());
        terminal.draw(|f| {
            render_bar(
                &input, cursor, hist_display, history_scroll,
                thinking, submit_anim.as_ref(),
                split_mode, &split_cmd, &split_args, split_cursor,
                hover_suggestion.as_deref(),
                &runspaces,
                &ext_ids,
                clr_progress,
                f,
            );
            if let Some(page) = help_page {
                help::render(page, &ext_entries, f);
            }
            if let Some(ref state) = ext_overlay {
                ext_browser::render(state, f);
            }
        })?;

        // Check if a background result or phase update has arrived.
        // Phase updates (gathering/resolving) keep thinking=true and update the
        // displayed message. Only a final (non-phase) message clears thinking.
        while let Ok((cmd, response)) = rx.try_recv() {
            let is_phase = matches!(
                response.as_str(),
                "resolving placement..." | "settling in..."
            );
            if let Some(last) = history.last_mut() {
                if last.0 == cmd {
                    last.1 = response.clone();
                }
            }
            if !is_phase {
                thinking = false;
                // If this was an Explore result, save it so `expand` can reference it.
                let was_explore = lodge_brain::intent::resolve_deterministic(&cmd).command
                    == lodge_brain::Command::Explore;
                if was_explore {
                    last_probe = Some(response);
                } else if cmd == "!ext" {
                    // Background install finished — refresh the installed indicators
                    // now that the binary actually exists on disk.
                    if let Some(ref mut s) = ext_overlay {
                        s.refresh_installed();
                    }
                }
            }
        }

        // Poll — 16 ms when typewriter/bar-fill is running, 50 ms otherwise (spinner is fine at 50ms).
        let poll_ms = if submit_anim.is_some() || needs_fast || clr_anim.is_some() { 16 } else { 50 };
        if !event::poll(std::time::Duration::from_millis(poll_ms))? {
            continue;
        }

        match event::read()? {
            Event::Paste(text) => {
                let cleaned: String = text
                    .chars()
                    .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                    .collect();
                let cleaned = cleaned.trim_end().to_string();
                if split_mode {
                    // Paste into args row
                    let chars: Vec<char> = split_args.chars().collect();
                    let before: String = chars[..split_cursor].iter().collect();
                    let after: String = chars[split_cursor..].iter().collect();
                    split_args = format!("{before}{cleaned}{after}");
                    split_cursor += cleaned.chars().count();
                } else {
                    let chars: Vec<char> = input.chars().collect();
                    let before: String = chars[..cursor].iter().collect();
                    let after: String = chars[cursor..].iter().collect();
                    input = format!("{before}{cleaned}{after}");
                    cursor += cleaned.chars().count();
                }
            }

            Event::Mouse(mouse) => {
                match mouse.kind {
                    // Track hover row for suggestion highlighting.
                    MouseEventKind::Moved => {
                        hover_row = Some(mouse.row);
                    }
                    // Left-click on a ✦ suggestion line fills the input field.
                    MouseEventKind::Down(MouseButton::Left) => {
                        let term_height = terminal.size().map(|r| r.height).unwrap_or(24);
                        if let Some(suggestion) = pick_suggestion(
                            mouse.row, term_height, &history, history_scroll, split_mode,
                        ) {
                            input = suggestion;
                            cursor = input.chars().count();
                            history_scroll = 0;
                        }
                    }
                    _ => {}
                }
            }

            Event::Key(key) => {
                // Ignore key-release and key-repeat — only act on press.
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // ── Help overlay key handling ──────────────────────────────
                if let Some(page) = help_page {
                    match key.code {
                        KeyCode::Left => {
                            help_page = Some(if page == 0 {
                                help::TOTAL_CARDS - 1
                            } else {
                                page - 1
                            });
                        }
                        KeyCode::Right | KeyCode::Tab => {
                            help_page = Some((page + 1) % help::TOTAL_CARDS);
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                            help_page = None;
                        }
                        _ => {}
                    }
                    continue;
                }

                // ── Extension browser overlay key handling ─────────────────
                if ext_overlay.is_some() {
                    // Phase 1 (immutable): capture action keys — blocked in search mode
                    // and when no visible selection exists.
                    let mut ext_action: Option<ext_browser::ExtAction> = None;
                    if let Some(ref state) = ext_overlay {
                        if !state.in_detail && !state.search_active {
                            if let Some(real_idx) = state.selected_real_idx() {
                                let sel_id        = state.entries[real_idx].id.clone();
                                let sel_installed = state.installed
                                    .get(real_idx).copied().unwrap_or(false);
                                match key.code {
                                    KeyCode::Char(' ') => {
                                        ext_action = Some(ext_browser::ExtAction::Toggle(sel_id));
                                    }
                                    KeyCode::Char('i') | KeyCode::Char('I') if !sel_installed => {
                                        ext_action = Some(ext_browser::ExtAction::Install(sel_id));
                                    }
                                    KeyCode::Char('u') | KeyCode::Char('U') if sel_installed => {
                                        ext_action = Some(ext_browser::ExtAction::Uninstall(sel_id));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    // Phase 2 (mutable): navigation, search input, and close.
                    if ext_action.is_none() {
                        if let Some(ref mut state) = ext_overlay {
                            match key.code {
                                // ── Search input ──────────────────────────────
                                KeyCode::Char('/') if !state.in_detail && !state.search_active => {
                                    state.search_active = true;
                                }
                                // Printable chars go to search when active
                                KeyCode::Char(c) if !state.in_detail && state.search_active => {
                                    state.search.push(c);
                                    state.selected = 0; // reset cursor on filter change
                                }
                                KeyCode::Backspace if !state.in_detail && state.search_active => {
                                    state.search.pop();
                                    if state.search.is_empty() {
                                        state.search_active = false;
                                    }
                                }
                                // Esc clears search if active, otherwise falls through to close
                                KeyCode::Esc if state.search_active => {
                                    state.search.clear();
                                    state.search_active = false;
                                }
                                // ── List navigation ───────────────────────────
                                KeyCode::Up if !state.in_detail && state.selected > 0 => {
                                    state.selected -= 1;
                                }
                                KeyCode::Down
                                    if !state.in_detail
                                        && state.selected + 1 < state.visible_count() =>
                                {
                                    state.selected += 1;
                                }
                                KeyCode::Enter
                                    if !state.in_detail
                                        && state.selected_real_idx().is_some() =>
                                {
                                    state.in_detail = true;
                                    state.sub_page  = 0;
                                }
                                // ── Detail navigation ─────────────────────────
                                KeyCode::Left if state.in_detail => {
                                    let total = state.sub_page_count();
                                    state.sub_page = if state.sub_page == 0 {
                                        total - 1
                                    } else {
                                        state.sub_page - 1
                                    };
                                }
                                KeyCode::Right | KeyCode::Tab if state.in_detail => {
                                    let total = state.sub_page_count();
                                    state.sub_page = (state.sub_page + 1) % total;
                                }
                                KeyCode::Esc | KeyCode::Char('b') | KeyCode::Char('B')
                                    if state.in_detail =>
                                {
                                    state.in_detail = false;
                                }
                                // ── Close ─────────────────────────────────────
                                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    ext_overlay = None;
                                }
                                _ => {}
                            }
                        }
                    }

                    // Execute the action (no borrow of ext_overlay held at this point).
                    if let Some(action) = ext_action {
                        match action {
                            ext_browser::ExtAction::Toggle(ref id) => {
                                if ext_state.enabled.contains(id) {
                                    ext_state.enabled.remove(id);
                                } else {
                                    ext_state.enabled.insert(id.clone());
                                }
                                crate::engine::extensions::save_ext_state(&ext_state);
                                // Rebuild autocomplete list to reflect new enabled state.
                                ext_ids = ext_entries.iter()
                                    .filter(|e| ext_state.enabled.contains(&e.id))
                                    .map(|e| e.command_alias().to_string())
                                    .collect();
                                // Sync state back into the overlay
                                if let Some(ref mut s) = ext_overlay {
                                    s.enabled = ext_state.enabled.clone();
                                    s.refresh_installed();
                                }
                                // Auto-install when just enabled and binary absent
                                if ext_state.enabled.contains(id)
                                    && !crate::engine::extensions::is_installed(id)
                                {
                                    if let Some(entry) =
                                        ext_entries.iter().find(|e| &e.id == id).cloned()
                                    {
                                        let id2  = id.clone();
                                        let tx2  = tx.clone();
                                        history.push((
                                            "!ext".into(),
                                            format!("installing {}…", id),
                                        ));
                                        thinking = true;
                                        std::thread::spawn(move || {
                                            let dir = crate::engine::extensions::extensions_dir();
                                            let res = crate::engine::extensions::install_extension(
                                                &entry, &dir,
                                            );
                                            let msg = match res {
                                                Ok(()) => format!("{id2} installed."),
                                                Err(e)  => format!("couldn't install {id2} — {e}"),
                                            };
                                            let _ = tx2.send(("!ext".into(), msg));
                                        });
                                    }
                                }
                            }
                            ext_browser::ExtAction::Install(ref id) => {
                                if let Some(entry) =
                                    ext_entries.iter().find(|e| &e.id == id).cloned()
                                {
                                    let id2 = id.clone();
                                    let tx2 = tx.clone();
                                    history.push(("!ext".into(), format!("installing {}…", id)));
                                    thinking = true;
                                    std::thread::spawn(move || {
                                        let dir = crate::engine::extensions::extensions_dir();
                                        let res = crate::engine::extensions::install_extension(
                                            &entry, &dir,
                                        );
                                        let msg = match res {
                                            Ok(()) => format!("{id2} installed."),
                                            Err(e)  => format!("couldn't install {id2} — {e}"),
                                        };
                                        let _ = tx2.send(("!ext".into(), msg));
                                    });
                                    // refresh_installed() will fire in try_recv when the
                                    // background thread sends its completion message.
                                } else {
                                    history.push((
                                        "!ext".into(),
                                        format!("no download URL for {}.", id),
                                    ));
                                }
                            }
                            ext_browser::ExtAction::Uninstall(ref id) => {
                                match crate::engine::extensions::uninstall_extension(id) {
                                    Ok(()) => {
                                        ext_state.enabled.remove(id);
                                        crate::engine::extensions::save_ext_state(&ext_state);
                                        // Rebuild autocomplete list.
                                        ext_ids = ext_entries.iter()
                                            .filter(|e| ext_state.enabled.contains(&e.id))
                                            .map(|e| e.command_alias().to_string())
                                            .collect();
                                        if let Some(ref mut s) = ext_overlay {
                                            s.enabled = ext_state.enabled.clone();
                                            s.refresh_installed();
                                        }
                                        history.push(("!ext".into(), format!("{} removed.", id)));
                                    }
                                    Err(e) => {
                                        history.push((
                                            "!ext".into(),
                                            format!("couldn't remove {} — {e}", id),
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    continue;
                }

                match (key.code, key.modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => break,

                    // ── Command history navigation (Up / Down) ───────────────
                    //
                    // Up/Down navigate the submitted-command ring exclusively.
                    // PgUp/PgDown scroll the history view.
                    (KeyCode::Up, _) if !cmd_history.is_empty() => {
                        let idx = cmd_history_idx
                            .map(|i| i.saturating_sub(1))
                            .unwrap_or(cmd_history.len() - 1);
                        cmd_history_idx = Some(idx);
                        input = cmd_history[idx].clone();
                        cursor = input.chars().count();
                    }

                    (KeyCode::Up, _) => {}

                    (KeyCode::Down, _) => {
                        if let Some(i) = cmd_history_idx {
                            if i + 1 < cmd_history.len() {
                                cmd_history_idx = Some(i + 1);
                                input = cmd_history[i + 1].clone();
                                cursor = input.chars().count();
                            } else {
                                // Past the newest — clear input, exit navigation.
                                cmd_history_idx = None;
                                input.clear();
                                cursor = 0;
                            }
                        }
                    }

                    // ── History view scroll (PgUp / PgDown) ──────────────────
                    //
                    // Scroll by ~half the visible area per keypress so one press
                    // moves a meaningful chunk without overshooting.
                    (KeyCode::PageUp, _) => {
                        let page = {
                            let size = terminal.size().unwrap_or_default();
                            let fixed = 5u16 + if split_mode { 1 } else { 0 };
                            size.height.saturating_sub(fixed) / 2
                        }.max(3);
                        history_scroll = history_scroll.saturating_add(page).min(max_history_scroll);
                    }

                    (KeyCode::PageDown, _) => {
                        let page = {
                            let size = terminal.size().unwrap_or_default();
                            let fixed = 5u16 + if split_mode { 1 } else { 0 };
                            size.height.saturating_sub(fixed) / 2
                        }.max(3);
                        history_scroll = history_scroll.saturating_sub(page);
                    }

                    (KeyCode::Left, _) => {
                        if split_mode {
                            split_cursor = split_cursor.saturating_sub(1);
                        } else {
                            cursor = cursor.saturating_sub(1);
                        }
                    }

                    (KeyCode::Right, _) => {
                        if split_mode {
                            let len = split_args.chars().count();
                            if split_cursor < len { split_cursor += 1; }
                        } else {
                            let len = input.chars().count();
                            if cursor < len {
                                cursor += 1;
                            } else if let Some(ghost) = ghost_completion(&input, &ext_ids) {
                                // Right at end of input fills in the ghost completion.
                                input.push_str(&ghost);
                                cursor = input.chars().count();
                            }
                        }
                    }

                    // ── Tab: cycle through visible suggestions ────────────────
                    (KeyCode::Tab, _) => {
                        // Collect suggestion texts from visible history lines.
                        let flat = build_flat_history_text(&history);
                        let suggestions: Vec<String> = flat
                            .iter()
                            .filter_map(|line| {
                                line.trim_start()
                                    .strip_prefix('✦')
                                    .map(|rest| rest.trim().to_string())
                            })
                            .collect();
                        if !suggestions.is_empty() {
                            let next_idx = tab_suggestion_idx
                                .map(|i| (i + 1) % suggestions.len())
                                .unwrap_or(0);
                            tab_suggestion_idx = Some(next_idx);
                            input = suggestions[next_idx].clone();
                            cursor = input.chars().count();
                        }
                    }

                    // ── Submit ────────────────────────────────────────────────
                    (KeyCode::Enter, _) => {
                        // Synthesise command from whichever mode is active.
                        let raw = if split_mode {
                            let a = split_args.trim().to_string();
                            let cmd = if a.is_empty() { split_cmd.clone() }
                                      else { format!("{} {a}", split_cmd) };
                            // Exit split mode
                            split_mode = false;
                            split_cmd.clear();
                            split_args.clear();
                            split_cursor = 0;
                            cmd
                        } else {
                            input.trim().to_string()
                        };
                        // Clear input, reset cursor and command-history navigation.
                        input.clear();
                        cursor = 0;
                        history_scroll = 0;
                        cmd_history_idx = None;
                        if raw.is_empty() || thinking {
                            continue;
                        }
                        // Record in command history (skip duplicates of the last entry).
                        if cmd_history.last().map(|s| s.as_str()) != Some(&raw) {
                            cmd_history.push(raw.clone());
                        }
                        // Strip leading ! — commands can be submitted from single-line mode
                        // without a space trigger (e.g. the user typed !help and hit Enter).
                        let raw = raw.trim_start_matches('!').to_string();
                        // Resolve synonyms: "get python" → dispatch "install python",
                        // animate "get" → "install" in the input row.
                        let (trimmed, anim_opt) =
                            if let Some((syn, can, sfx)) = find_synonym(&raw) {
                                let canonical = format!("{can}{sfx}");
                                let a = SynonymAnim::new(&syn, &can, &sfx);
                                (canonical, Some(a))
                            } else {
                                (raw, None)
                            };
                        submit_anim = anim_opt;

                        {
                            // Auto-clear registered directories on each prompt that is not
                            // itself a register or expand command.
                            if register_active
                                && trimmed != "register"
                                && !trimmed.starts_with("register ")
                                && !trimmed.starts_with("expand")
                            {
                                registered_paths.clear();
                                register_active = false;
                            }

                            // ── Clr: clear chat history (wipe-up animation) ──────────────
                            if trimmed == "clr" {
                                // Wipe the terminal immediately so ratatui's
                                // buffer is fully reset before the first
                                // animation frame — prevents any pre-existing
                                // on-screen content from surviving as ghost text.
                                terminal.clear()?;
                                clr_anim = Some(ClrAnim::new(std::mem::take(&mut history)));
                                history_scroll = 0;
                                continue;
                            }

                            // ── Active: add probe result to band ────────────────────────
                            if trimmed == "active" || trimmed.starts_with("active ") {
                                let args_str = trimmed.trim_start_matches("active").trim().to_string();

                                if args_str == "clear" {
                                    runspaces.clear();
                                    history.push((trimmed, "band cleared.".into()));
                                    continue;
                                }

                                if runspaces.len() >= 5 {
                                    history.push((trimmed, "band is full — 5 runspaces maximum. type `active clear` to reset.".into()));
                                    continue;
                                }

                                if args_str.is_empty() {
                                    // Use last probe result
                                    if let Some(probe_text) = &last_probe {
                                        let rs = Runspace::new("last probe", probe_text.clone());
                                        runspaces.push(rs);
                                        history.push((trimmed, "probe added to band.".into()));
                                    } else {
                                        history.push((trimmed, "no probe result to display. run a system probe first.".into()));
                                    }
                                    continue;
                                }

                                // Parse: first word is probe name, rest are positional args
                                let words: Vec<&str> = args_str.splitn(10, ' ').collect();
                                let probe_name = words[0].to_string();

                                if let Some(probe_def) = lodge_brain::scout::PROBES.iter().find(|p| p.name == probe_name) {
                                    let mut probe_args: std::collections::HashMap<String, String> = Default::default();
                                    // Map positional args to declared arg names
                                    for (i, key) in probe_def.args.iter().enumerate() {
                                        if let Some(val) = words.get(i + 1) {
                                            probe_args.insert(key.to_string(), val.to_string());
                                        }
                                    }
                                    // Run probe synchronously (fast probes)
                                    match lodge_brain::scout::dispatch(&probe_name, &probe_args) {
                                        Some(result) => {
                                            let value = result.value
                                                .or(result.raw)
                                                .unwrap_or_else(|| result.error.unwrap_or_else(|| "no result".into()));
                                            let rs = Runspace::new(probe_name.clone(), value.clone())
                                                .with_live(probe_name.clone(), probe_args);
                                            runspaces.push(rs);
                                            history.push((trimmed, format!("{probe_name} added to band.")));
                                        }
                                        None => {
                                            history.push((trimmed, format!("probe '{probe_name}' not found.")));
                                        }
                                    }
                                } else {
                                    let available: Vec<&str> = lodge_brain::scout::PROBES.iter().map(|p| p.name).take(8).collect();
                                    history.push((trimmed, format!("unknown probe '{probe_name}'. available: {}", available.join(", "))));
                                }
                                continue;
                            }

                            // ── Register: probe registered directories ───────────────────
                            if trimmed == "register" || trimmed.starts_with("register ") {
                                let args = trimmed.trim_start_matches("register").trim().to_string();
                                if args.is_empty() {
                                    let msg = if registered_paths.is_empty() {
                                        "no directories registered.\n  usage: !register <path> [<path>...]\n  probes each path and stores the result for !expand.".into()
                                    } else {
                                        let mut m = format!("{} path(s) active:", registered_paths.len());
                                        for p in &registered_paths { m.push_str(&format!("\n  {p}")); }
                                        m
                                    };
                                    history.push((trimmed, msg));
                                } else {
                                    let has_files_flag = args.starts_with("--files ") || args == "--files";
                                    let clean_args = args.trim_start_matches("--files").trim().to_string();
                                    let raw_paths = parse_paths(&clean_args);
                                    let expanded: Vec<String> = raw_paths.iter()
                                        .map(|p| expand_env_path(p))
                                        .collect();
                                    registered_paths = expanded.clone();
                                    let mut sections: Vec<String> = Vec::new();
                                    for p in &expanded {
                                        sections.push(probe_directory(p, has_files_flag));
                                    }
                                    let msg = sections.join("\n\n");
                                    // Store so !expand can analyse it immediately.
                                    last_probe = Some(msg.clone());
                                    register_active = true;
                                    history.push((trimmed, msg));
                                }
                                continue;
                            }

                            // ── Key: AI provider management ─────────────────────────────
                            if trimmed == "key" || trimmed.starts_with("key ") {
                                let args = trimmed.trim_start_matches("key").trim();
                                let msg = if args.is_empty() {
                                    // Show current provider.
                                    let (provider, model, _) = lodge_brain::ai::resolve_provider();
                                    if provider == lodge_brain::ai::Provider::None {
                                        "no AI provider configured. use `!key set <api-key>` or `!key ollama`.".into()
                                    } else {
                                        format!("AI provider: {}  ({})", provider.label(), model)
                                    }
                                } else if let Some(api_key) = args.strip_prefix("set ") {
                                    let api_key = api_key.trim();
                                    if api_key.is_empty() {
                                        "usage: !key set <api-key>".into()
                                    } else {
                                        match lodge_brain::ai::save_key(api_key) {
                                            Ok(provider) => format!("{} key saved.", provider.label()),
                                            Err(e) => format!("couldn't save key — {e}"),
                                        }
                                    }
                                } else if args == "ollama" {
                                    match lodge_brain::ai::save_ollama() {
                                        Ok(()) => "Ollama set as AI provider.".into(),
                                        Err(e) => format!("couldn't save config — {e}"),
                                    }
                                } else if args == "clear" {
                                    match lodge_brain::ai::clear_config() {
                                        Ok(()) => "AI config cleared.".into(),
                                        Err(e) => format!("couldn't clear config — {e}"),
                                    }
                                } else {
                                    "usage: !key  |  !key set <api-key>  |  !key ollama  |  !key clear".into()
                                };
                                history.push((trimmed, msg));
                                continue;
                            }

                            // ── Ollama model management ──────────────────────────────────
                            if trimmed == "ollama" || trimmed.starts_with("ollama ") {
                                let args = trimmed.trim_start_matches("ollama").trim().to_string();
                                let msg = handle_ollama_command(&args);
                                history.push((trimmed, msg));
                                continue;
                            }

                            // ── Dynamic extension dispatch ───────────────────────────────
                            let ext_match: Option<crate::engine::extensions::RegistryEntry> = {
                                let (entries, _) = crate::engine::extensions::fetch_registry();
                                let first_word = trimmed.split_whitespace().next().unwrap_or(&trimmed);
                                entries.into_iter().find(|e| e.command_alias() == first_word)
                            };

                            if let Some(ref ext_entry) = ext_match {
                                // Respect the enabled/disabled toggle from !ext.
                                if !ext_state.enabled.contains(&ext_entry.id) {
                                    history.push((
                                        trimmed,
                                        format!(
                                            "{} is disabled — open !ext to enable it.",
                                            ext_entry.name
                                        ),
                                    ));
                                    input.clear();
                                    cursor = 0;
                                    continue;
                                }

                                let alias = ext_entry.command_alias().to_string();
                                // User-supplied path after the alias (e.g. `!clean ~/Downloads`).
                                let user_path = trimmed.trim_start_matches(&*alias).trim().to_string();
                                match show_extension_card(ext_entry, &mut terminal) {
                                    Ok(true) => {
                                        history.push((trimmed.clone(), format!("stepping into {}...", ext_entry.name)));
                                        // Prefer user-supplied path, then home dir.
                                        // Never pass CWD — it's usually the Lodge source tree.
                                        let scan_path: String = if !user_path.is_empty() {
                                            user_path.clone()
                                        } else {
                                            #[cfg(windows)]
                                            { std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string()) }
                                            #[cfg(not(windows))]
                                            { std::env::var("HOME").unwrap_or_else(|_| ".".to_string()) }
                                        };
                                        match launch_extension(&ext_entry.id, &scan_path, &mut terminal) {
                                            Ok(()) => {
                                                history.push((String::new(), format!("{} — done.", ext_entry.name)));
                                            }
                                            Err(e) => {
                                                history.push((String::new(), format!("couldn't launch {} — {e}", ext_entry.name)));
                                            }
                                        }
                                    }
                                    Ok(false) => {
                                        history.push((trimmed.clone(), "stayed here.".into()));
                                    }
                                    Err(e) => {
                                        history.push((trimmed.clone(), format!("extension error — {e}")));
                                    }
                                }
                                input.clear();
                                cursor = 0;
                                continue;
                            }

                            // ── Extension browser ─────────────────────────────────────────
                            if trimmed == "ext" {
                                if ext_entries.is_empty() {
                                    history.push((trimmed, "no extensions available.".into()));
                                } else {
                                    ext_overlay = Some(ext_browser::ExtBrowserState::new(
                                        ext_entries.clone(),
                                        ext_state.enabled.clone(),
                                        ext_registry_online,
                                        ext_official_ids.clone(),
                                    ));
                                }
                                continue;
                            }

                            // ── Help: route to the appropriate card ──────────────────────
                            if trimmed == "help" || trimmed.starts_with("help ") {
                                let topic = trimmed.trim_start_matches("help").trim().to_string();
                                help_page = Some(help_page_for_topic(&topic));
                                continue;
                            }

                            let intent = lodge_brain::intent::resolve_deterministic(&trimmed);

                            // ── Install: show flashcard synchronously, then install async ──
                            if matches!(intent.command, Command::Install) {
                                let target = intent
                                    .args
                                    .get("target")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(trimmed.trim_start_matches("install").trim())
                                    .trim()
                                    .to_string();

                                if target.is_empty() {
                                    history.push((trimmed, "install what? try: install <id>".into()));
                                } else {
                                    // Drain any active synonym animation before the flashcard takes
                                    // over the terminal — otherwise "get" → "install" never plays out.
                                    while submit_anim.is_some() {
                                        let ts = terminal.size().unwrap_or_default();
                                        let hs_anim = compute_hover_suggestion(
                                            hover_row, &history, history_scroll, split_mode, ts.height,
                                        );
                                        terminal.draw(|f| {
                                            render_bar(
                                                &input, cursor, &history, history_scroll,
                                                false, submit_anim.as_ref(),
                                                split_mode, &split_cmd, &split_args, split_cursor,
                                                hs_anim.as_deref(),
                                                &runspaces,
                                                &ext_ids,
                                                None,
                                                f,
                                            );
                                        })?;
                                        let done = submit_anim.as_mut().map(|a| a.tick()).unwrap_or(true);
                                        if done {
                                            submit_anim = None;
                                        } else {
                                            std::thread::sleep(std::time::Duration::from_millis(
                                                SynonymAnim::STEP_MS,
                                            ));
                                        }
                                    }
                                    match crate::engine::feed::find_latest(&target) {
                                        None => {
                                            // Not in local feed — try Scoop as a fallback.
                                            history.push((trimmed.clone(), "checking Scoop...".into()));
                                            let ts2 = terminal.size().unwrap_or_default();
                                            let hs_scoop = compute_hover_suggestion(
                                                hover_row, &history, history_scroll, split_mode, ts2.height,
                                            );
                                            terminal.draw(|f| {
                                                render_bar(
                                                    &input, cursor, &history, history_scroll,
                                                    false, None,
                                                    split_mode, &split_cmd, &split_args, split_cursor,
                                                    hs_scoop.as_deref(),
                                                    &runspaces,
                                                    &ext_ids,
                                                    None,
                                                    f,
                                                );
                                            })?;
                                            match crate::engine::scoop::fetch(&target) {
                                                Ok(pkg) => {
                                                    history.pop(); // remove "checking Scoop..."
                                                    let manifest = pkg.to_lodge_manifest();
                                                    let plan = pkg.to_placement_plan();
                                                    match flashcard::show(&manifest, &plan, &mut terminal) {
                                                        Ok(true) => {
                                                            history.push((trimmed.clone(), "fetching from Scoop...".into()));
                                                            thinking = true;
                                                            let tx2 = tx.clone();
                                                            let input_clone = trimmed.clone();
                                                            std::thread::spawn(move || {
                                                                let response = match crate::engine::scoop::install(&pkg, lodge::VERSION) {
                                                                    Ok(r) => format!("{} v{} settled in.", r.id, r.version),
                                                                    Err(e) => format!("couldn't install: {e}"),
                                                                };
                                                                let _ = tx2.send((input_clone, response));
                                                            });
                                                        }
                                                        Ok(false) => {
                                                            history.push((trimmed, "left where it was.".into()));
                                                        }
                                                        Err(e) => {
                                                            history.push((trimmed, format!("error: {e}")));
                                                        }
                                                    }
                                                }
                                                Err(_) => {
                                                    if let Some(last) = history.last_mut() {
                                                        last.1 = format!("'{target}' not found in the local feed or Scoop.");
                                                    }
                                                }
                                            }
                                        }
                                        Some(entry) => {
                                            let pkg_path = entry.path.clone();
                                            match load_manifest_and_plan(&pkg_path) {
                                                Err(e) => {
                                                    history.push((trimmed, format!("couldn't read package: {e}")));
                                                }
                                                Ok((manifest, plan)) => {
                                                    match flashcard::show(&manifest, &plan, &mut terminal) {
                                                        Ok(true) => {
                                                            // User confirmed — install in background
                                                            history.push((trimmed.clone(), "settling in...".into()));
                                                            thinking = true;
                                                            let tx2 = tx.clone();
                                                            let input_clone = trimmed.clone();
                                                            std::thread::spawn(move || {
                                                                let response = match crate::engine::installer::silent_install(&pkg_path, lodge::VERSION) {
                                                                    Ok(r) => format!("{} v{} settled in.", r.id, r.version),
                                                                    Err(e) => format!("couldn't install: {e}"),
                                                                };
                                                                let _ = tx2.send((input_clone, response));
                                                            });
                                                        }
                                                        Ok(false) => {
                                                            history.push((trimmed, "left where it was.".into()));
                                                        }
                                                        Err(e) => {
                                                            history.push((trimmed, format!("error: {e}")));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                input.clear();
                                continue;
                            }

                            // Commands that run in a background thread with animated state:
                            // - Explore: system probes that may invoke PowerShell (slow)
                            // - Clarify + model: model inference
                            let is_explore = matches!(intent.command, Command::Explore);
                            let is_expand  = matches!(intent.command, Command::Expand);
                            let is_scan    = matches!(intent.command, Command::Scan);
                            let needs_async = is_explore
                                || is_expand
                                || is_scan
                                || (matches!(intent.command, Command::Clarify) && has_model);

                            if needs_async {
                                let initial_msg = if is_explore {
                                    "probing system..."
                                } else if is_scan {
                                    "scanning system..."
                                } else if is_expand {
                                    "asking AI..."
                                } else {
                                    "thinking..."
                                };
                                history.push((trimmed.clone(), initial_msg.into()));
                                thinking = true;

                                let brain_ref = Arc::clone(&brain);
                                let input_clone = trimmed.clone();
                                let tx2 = tx.clone();
                                // Capture the last probe result for `expand`
                                let probe_ctx = last_probe.clone().unwrap_or_default();
                                std::thread::spawn(move || {
                                    if is_scan {
                                        let response = lodge_brain::scan::run_with_narration();
                                        let _ = tx2.send((input_clone, response));
                                    } else if is_expand {
                                        let question = input_clone.split_once(' ').map(|x| x.1);
                                        if probe_ctx.is_empty() {
                                            let _ = tx2.send((
                                                input_clone,
                                                "no probe result to expand. ask a system question first.".into(),
                                            ));
                                        } else {
                                            let response = lodge_brain::ai::expand(&probe_ctx, question);
                                            let _ = tx2.send((input_clone, response));
                                        }
                                    } else {
                                        // Explore (probes) and Clarify+model both go through brain.handle,
                                        // which updates conversation context internally.
                                        let response =
                                            brain_ref.lock().unwrap().handle(&input_clone);
                                        let _ = tx2.send((input_clone, response));
                                    }
                                });
                            } else {
                                let mut b = brain.lock().unwrap();
                                let response = handle_command(&mut b, &trimmed, &ext_ids);
                                // Push to conversation context so follow-up questions can
                                // reference what was just asked and answered.
                                b.context.push(trimmed.clone(), response.clone());
                                history.push((trimmed, response));
                            }
                        } // end inner intent block
                    }

                    // ── Editing ───────────────────────────────────────────────
                    (KeyCode::Backspace, _) => {
                        if split_mode {
                            if split_cursor > 0 {
                                let chars: Vec<char> = split_args.chars().collect();
                                let before: String = chars[..split_cursor - 1].iter().collect();
                                let after: String = chars[split_cursor..].iter().collect();
                                split_args = format!("{before}{after}");
                                split_cursor -= 1;
                            } else {
                                // Backspace on empty args row → cancel split mode
                                input = format!("!{}", split_cmd);
                                cursor = input.chars().count();
                                split_mode = false;
                                split_cmd.clear();
                                split_args.clear();
                                split_cursor = 0;
                            }
                        } else if cursor > 0 {
                            let chars: Vec<char> = input.chars().collect();
                            let before: String = chars[..cursor - 1].iter().collect();
                            let after: String = chars[cursor..].iter().collect();
                            input = format!("{before}{after}");
                            cursor -= 1;
                        }
                    }

                    (KeyCode::Char(c), _) => {
                        cmd_history_idx = None; // typing breaks out of history navigation
                        tab_suggestion_idx = None; // typing resets tab navigation
                        if split_mode {
                            // Insert into args row
                            let chars: Vec<char> = split_args.chars().collect();
                            let before: String = chars[..split_cursor].iter().collect();
                            let after: String = chars[split_cursor..].iter().collect();
                            split_args = format!("{before}{c}{after}");
                            split_cursor += 1;
                        } else {
                            // Insert into main input
                            let chars: Vec<char> = input.chars().collect();
                            let before: String = chars[..cursor].iter().collect();
                            let after: String = chars[cursor..].iter().collect();
                            input = format!("{before}{c}{after}");
                            cursor += 1;
                            // After inserting, check for `!command ` trigger → enter split mode.
                            if c == ' ' {
                                if let Some(cmd_part) = input.trim().strip_prefix('!') {
                                    let words: Vec<&str> = cmd_part.split_whitespace().collect();
                                    if let Some((canonical, CmdKind::Command, _)) = detect_trigger(&words) {
                                        split_mode = true;
                                        split_cmd = canonical;
                                        split_args.clear();
                                        split_cursor = 0;
                                        input.clear();
                                        cursor = 0;
                                    }
                                }
                            }
                        }
                    }

                    _ => {}
                }
            }

            _ => {}
        }
    }

    // Restore terminal and clear the custom window title.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableMouseCapture,
        SetTitle("")
    )?;
    Ok(())
}

/// Routes a command through the brain, with runtime-layer overrides for
/// commands that need filesystem access or shim manipulation.
fn handle_command(brain: &mut Brain, input: &str, ext_ids: &[String]) -> String {
    let intent = lodge_brain::intent::resolve_deterministic(input);
    match intent.command {
        Command::Help => {
            let mut help = lodge_brain::framer::HELP.to_string();
            if !ext_ids.is_empty() {
                help.push_str("\n\nextensions:");
                for id in ext_ids {
                    help.push_str(&format!("\n  !{id:<18}  run the {id} extension"));
                }
            }
            help.push_str("\n\nbuilt-in tools:");
            help.push_str("\n  !ext                open the extension browser");
            help.push_str("\n  !ollama             manage local Ollama models");
            help.push_str("\n  !scan               full system probe battery");
            help.push_str("\n  !register <path>    probe a directory");
            help
        }

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
             community ruleset updates are not yet available."
                .into()
        }

        // Explore routes to system probes — no model needed
        Command::Explore => brain.handle(input),

        // Scan and Expand are always dispatched async so they never hit this branch.
        Command::Scan => lodge_brain::scan::run_with_narration(),
        Command::Expand => {
            "run a probe first, then type `expand` to go deeper.".into()
        }

        // Clarify means the input wasn't recognised. With a model this is handled
        // async in run(); without one, give a calm redirect.
        Command::Clarify => {
            "not a recognised command. type help to see what's available.".into()
        }
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
    match crate::engine::feed::find_latest(target) {
        None => format!(
            "'{target}' not found in the local feed. \
             use `lodge install {target}` from the terminal for path-based installs."
        ),
        Some(entry) => {
            match crate::engine::installer::silent_install(&entry.path, lodge::VERSION) {
                Ok(receipt) => format!("{} v{} settled in.", receipt.id, receipt.version),
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

/// Loads a package manifest and computes its placement plan without executing anything.
///
/// Used by the flashcard path so the pre-install summary can be shown before
/// any files are touched.
fn load_manifest_and_plan(
    pkg_path: &std::path::Path,
) -> anyhow::Result<(
    lodge_shared::manifest::Manifest,
    lodge_shared::placement::PlacementPlan,
)> {
    let json = std::fs::read_to_string(pkg_path.join("lodge.json"))
        .map_err(|e| anyhow::anyhow!("couldn't read lodge.json: {e}"))?;
    let manifest = crate::engine::manifest::parse(&json)?;
    let os = crate::engine::resolver::current_os();
    let plan = crate::engine::resolver::resolve(pkg_path, &manifest, os, false)?;
    Ok((manifest, plan))
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
    let mut seen = std::collections::HashSet::new();
    let lines: Vec<String> = receipts
        .into_iter()
        .filter(|r| seen.insert(r.id.clone()))
        .map(|r| format!("  {}  v{}", r.id, r.version))
        .collect();
    lines.join("\n")
}

/// Apply the right-to-left clear animation to an already-built `Vec<Line>`.
///
/// `progress` 0.0–1.0 → Phase 1: sweep ░ in from the right, replacing original text.
/// `progress` 1.0–2.0 → Phase 2: sweep spaces in from the right, erasing the ░.
/// `area_width` is the display-column width of the history pane.
fn apply_clr_mask(
    lines:      Vec<Line<'_>>,
    progress:   f32,
    area_width: usize,
) -> Vec<Line<'static>> {
    let phase2      = progress >= 1.0;
    let frac        = if phase2 { progress - 1.0 } else { progress }.clamp(0.0, 1.0);
    // boundary_col: chars at col >= boundary_col are already transformed.
    // Starts at area_width (nothing changed) and sweeps to 0 (everything changed).
    let boundary_col = ((1.0 - frac) * area_width as f32).round() as usize;
    let noise_style  = Style::default().fg(palette::TEXT_DIM);

    lines
        .into_iter()
        .map(|line| {
            let total: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();

            if phase2 {
                // Phase 2: ░ blocks remain left of boundary, spaces clear the right.
                let noise   = boundary_col.min(total);
                let cleared = total.saturating_sub(boundary_col);
                let mut spans: Vec<Span<'static>> = Vec::with_capacity(2);
                if noise > 0 {
                    spans.push(Span::styled("░".repeat(noise), noise_style));
                }
                if cleared > 0 {
                    // Explicit spaces so ratatui's diff overwrites old ░ cells.
                    spans.push(Span::styled(" ".repeat(cleared), Style::default()));
                }
                Line::from(spans)
            } else {
                // Phase 1: keep original styled spans left of boundary; ░ right of it.
                let mut col       = 0usize;
                let mut new_spans: Vec<Span<'static>> = Vec::new();

                for span in line.spans {
                    let style = span.style;
                    let mut before      = String::new();
                    let mut noise_count = 0usize;

                    for c in span.content.chars() {
                        if col < boundary_col {
                            before.push(c);
                        } else {
                            noise_count += 1;
                        }
                        col += 1;
                    }
                    if !before.is_empty() {
                        new_spans.push(Span::styled(before, style));
                    }
                    if noise_count > 0 {
                        new_spans.push(Span::styled("░".repeat(noise_count), noise_style));
                    }
                }
                Line::from(new_spans)
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn render_bar(
    input:            &str,
    cursor:           usize,
    history:          &[(String, String)],
    history_scroll:   u16,
    thinking:         bool,
    submit_anim:      Option<&SynonymAnim>,
    split_mode:       bool,
    split_cmd:        &str,
    split_args:       &str,
    split_cursor:     usize,
    hover_suggestion: Option<&str>,
    runspaces:        &[Runspace],
    ext_ids:          &[String],
    clr_progress:     Option<f32>,
    frame:            &mut ratatui::Frame,
) {
    let area = frame.area();

    // ── Right-hand band layout ─────────────────────────────────────────────────
    // Band is 30 cols wide; shown when terminal is at least 100 cols so the main
    // area still has at least 69 columns (100 - 30 - 1 separator).
    let show_band = area.width >= 100 && !runspaces.is_empty();
    let band_w = 30u16;
    // Band is constrained to the history content rows (after header+divider, before
    // mid-divider+input+bottom-divider) so it never overlaps chrome and stays visually fixed.
    let input_rows = if split_mode { 2u16 } else { 1u16 };
    let band_y      = area.y + 2;                               // skip header + top divider
    let band_h      = area.height.saturating_sub(4 + input_rows); // minus header, 2 divs, input
    let (main_area, band_area) = if show_band {
        let mw = area.width - band_w - 1;
        let ma = ratatui::layout::Rect { width: mw, ..area };
        let ba = ratatui::layout::Rect {
            x: area.x + mw + 1,
            y: band_y,
            width: band_w,
            height: band_h,
        };
        (ma, Some(ba))
    } else {
        (area, None)
    };

    let div  = "─".repeat(main_area.width as usize);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),           // header
            Constraint::Length(1),           // top divider
            Constraint::Min(0),              // history
            Constraint::Length(1),           // mid divider
            Constraint::Length(input_rows),  // input section
            Constraint::Length(1),           // bottom divider
        ])
        .split(main_area);

    // ── Header ────────────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  lodge", Style::default().fg(palette::ACCENT)),
            Span::styled(
                "  ·  a place for everything",
                Style::default().fg(palette::TEXT_DIM),
            ),
        ])),
        chunks[0],
    );

    let div_span = Span::styled(&div, Style::default().fg(palette::BORDER));
    frame.render_widget(Paragraph::new(Line::from(div_span.clone())), chunks[1]);

    // ── History ───────────────────────────────────────────────────────────────
    // Build (is_suggestion, display_text, line) so hover highlighting can be
    // applied after the layout geometry is known.
    let last_idx = history.len().saturating_sub(1);
    // (is_suggestion, raw_text_for_hover_rebuild, line)
    let mut history_data: Vec<(bool, String, Line)> = Vec::new();

    for (idx, (cmd, resp)) in history.iter().enumerate() {
        if !cmd.is_empty() {
            let display_cmd: String = if idx == last_idx {
                if let Some(anim) = submit_anim { anim.display() } else { cmd.clone() }
            } else {
                cmd.clone()
            };
            let kind = classify_kind(cmd);
            let (prefix, cmd_text, cmd_color) = match kind {
                CmdKind::Command   => ("  > ", format!("!{display_cmd}"), palette::IN_PROGRESS),
                CmdKind::Directive => ("  > ", display_cmd,               palette::HIGHLIGHT),
                CmdKind::None      => ("  > ", display_cmd,               palette::TEXT),
            };
            history_data.push((false, String::new(), Line::from(vec![
                Span::styled(prefix, Style::default().fg(palette::ACCENT)),
                Span::styled(cmd_text, Style::default().fg(cmd_color)),
            ])));
        }
        if !resp.is_empty() {
            let is_progress = matches!(
                resp.as_str(),
                "thinking..." | "gathering resources..."
                | "resolving placement..." | "settling in..."
                | "probing system..." | "scanning system..." | "asking AI..."
            );
            let resp_style = if is_progress {
                Style::default().fg(palette::IN_PROGRESS)
            } else {
                Style::default().fg(palette::TEXT_DIM)
            };
            for part in resp.split('\n') {
                for wrapped in wrap_at(part, 98) {
                    let is_suggestion = wrapped.trim_start().starts_with('✦');
                    let text = format!("  {wrapped}");
                    let line_style = if is_suggestion {
                        Style::default().fg(palette::HIGHLIGHT)
                    } else {
                        resp_style
                    };
                    history_data.push((
                        is_suggestion,
                        text.clone(),
                        Line::from(Span::styled(text, line_style)),
                    ));
                }
            }
            history_data.push((false, String::new(), Line::from("")));
        }
    }

    let history_height   = history_data.len() as u16;
    let available_height = chunks[2].height;

    // Build final Vec<Line>, applying hover style to suggestions matching hover_suggestion.
    // `text` is "  ✦ suggestion text", `hover_suggestion` is "suggestion text" — strip ✦ prefix.
    let history_lines: Vec<Line> = history_data
        .into_iter()
        .map(|(is_sug, text, line)| {
            let hovered = is_sug && hover_suggestion.is_some_and(|hs| {
                text.trim_start()
                    .strip_prefix('✦')
                    .map(|s| s.trim())
                    == Some(hs.trim())
            });
            if hovered {
                Line::from(Span::styled(
                    text,
                    Style::default().fg(palette::SURFACE).bg(palette::HIGHLIGHT),
                ))
            } else {
                line
            }
        })
        .collect();

    if let Some(p) = clr_progress {
        // ── Animation path ────────────────────────────────────────────────────
        // Render a single Paragraph that covers every cell in chunks[2].
        // Padding rows at the top + per-line right-padding ensure no cell is
        // left unwritten — eliminating ghost characters without relying on
        // ratatui's diff to catch unwritten cells.
        let area_w = chunks[2].width as usize;
        let avail  = available_height as usize;
        let masked = apply_clr_mask(history_lines, p, area_w);

        // Overflow: take only the bottom `avail` lines.
        let skip    = masked.len().saturating_sub(avail);
        let visible: Vec<Line<'static>> = masked.into_iter().skip(skip).collect();

        // Top-padding: explicit full-width space rows to overwrite old content.
        let pad    = avail.saturating_sub(visible.len());
        let bg_row = || Line::from(Span::raw(" ".repeat(area_w)));
        let mut all_lines: Vec<Line<'static>> = (0..pad).map(|_| bg_row()).collect();

        for line in visible {
            // Right-pad each content line so every cell in the row is written.
            let line_w: usize = line.spans.iter()
                .map(|s| s.content.chars().count())
                .sum();
            let mut line = line;
            if line_w < area_w {
                line.spans.push(Span::raw(" ".repeat(area_w - line_w)));
            }
            all_lines.push(line);
        }

        frame.render_widget(Paragraph::new(all_lines), chunks[2]);
    } else {
        // ── Normal path ───────────────────────────────────────────────────────
        // Clear chunks[2] every frame so cells above hist_area are erased when
        // history shrinks.
        frame.render_widget(Clear, chunks[2]);
        frame.render_widget(
            Block::default().style(Style::default().bg(palette::BG)),
            chunks[2],
        );

        if history_height > 0 {
            if history_height <= available_height {
                // History fits — bottom-align it.
                let hist_area = ratatui::layout::Rect {
                    x:      chunks[2].x,
                    y:      chunks[2].bottom() - history_height,
                    width:  chunks[2].width,
                    height: history_height,
                };
                frame.render_widget(Paragraph::new(history_lines), hist_area);
            } else {
                // History overflows — apply user scroll offset (0 = bottom).
                let base   = history_height - available_height;
                let offset = base.saturating_sub(history_scroll);
                let para_area = ratatui::layout::Rect {
                    width: chunks[2].width.saturating_sub(1),
                    ..chunks[2]
                };
                frame.render_widget(
                    Paragraph::new(history_lines).scroll((offset, 0)),
                    para_area,
                );
                // Scrollbar: content_length = total lines, viewport = visible lines,
                // position = top line currently shown (0 = content top, base = content bottom).
                let mut sb_state = ScrollbarState::new(history_height as usize)
                    .viewport_content_length(available_height as usize)
                    .position(offset as usize);
                frame.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(None)
                        .end_symbol(None)
                        .thumb_style(Style::default().fg(palette::TEXT_DIM))
                        .track_style(Style::default().fg(palette::SURFACE)),
                    chunks[2],
                    &mut sb_state,
                );
            }
        }
    }

    frame.render_widget(Paragraph::new(Line::from(div_span.clone())), chunks[3]);

    // ── Input section ─────────────────────────────────────────────────────────
    let input_area = chunks[4];

    if split_mode {
        // Two rows: command label on top, args prompt below.
        let cmd_row  = ratatui::layout::Rect { height: 1, ..input_area };
        let args_row = ratatui::layout::Rect { y: input_area.y + 1, height: 1, ..input_area };

        // Top row — `! <command>` in command colour
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  ! ", Style::default().fg(palette::ACCENT)),
                Span::styled(split_cmd.to_string(), Style::default().fg(palette::IN_PROGRESS)),
            ])),
            cmd_row,
        );

        // Bottom row — args input with cursor
        let chars: Vec<char> = split_args.chars().collect();
        let before: String = chars[..split_cursor].iter().collect();
        let at_cur: String = chars.get(split_cursor).copied().unwrap_or('_').to_string();
        let after: String  = if split_cursor < chars.len() {
            chars[split_cursor + 1..].iter().collect()
        } else { String::new() };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  > ", Style::default().fg(palette::ACCENT)),
                Span::styled(before, Style::default().fg(palette::TEXT)),
                Span::styled(at_cur, Style::default().fg(palette::HIGHLIGHT)),
                Span::styled(after, Style::default().fg(palette::TEXT)),
            ])),
            args_row,
        );
    } else {
        // Single-row free input — animation (if running) handled in history above.
        let prompt_line = if thinking && submit_anim.is_none() {
            Line::from(vec![
                Span::styled("  > ", Style::default().fg(palette::ACCENT)),
                Span::styled("thinking...", Style::default().fg(palette::IN_PROGRESS)),
            ])
        } else {
            // Real-time color classification
            let input_color = match classify_kind(input) {
                CmdKind::Command   => palette::IN_PROGRESS,
                CmdKind::Directive => palette::HIGHLIGHT,
                CmdKind::None      => palette::TEXT,
            };
            // Ghost autocomplete when cursor is at the end and typing a ! command.
            let ghost = if cursor == input.chars().count() {
                ghost_completion(input, ext_ids)
            } else {
                None
            };
            // Cursor rendering
            let chars: Vec<char> = input.chars().collect();
            let before: String = chars[..cursor].iter().collect();
            let at_cur: String = chars.get(cursor).copied().unwrap_or('_').to_string();
            let after: String  = if cursor < chars.len() {
                chars[cursor + 1..].iter().collect()
            } else { String::new() };
            let mut input_spans = vec![
                Span::styled("  > ", Style::default().fg(palette::ACCENT)),
                Span::styled(before, Style::default().fg(input_color)),
                Span::styled(at_cur, Style::default().fg(palette::HIGHLIGHT)),
                Span::styled(after, Style::default().fg(input_color)),
            ];
            if let Some(g) = ghost {
                input_spans.push(Span::styled(g, Style::default().fg(palette::TEXT_DIM)));
            }
            Line::from(input_spans)
        };
        frame.render_widget(Paragraph::new(prompt_line), input_area);
    }

    frame.render_widget(Paragraph::new(Line::from(div_span)), chunks[5]);

    // ── Right-hand band ───────────────────────────────────────────────────────
    if let Some(ba) = band_area {
        // Draw separator column — full terminal height so it meets the horizontal dividers.
        for row in area.y..area.y + area.height {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "│",
                    Style::default().fg(palette::BORDER),
                ))),
                ratatui::layout::Rect { x: ba.x - 1, y: row, width: 1, height: 1 },
            );
        }
        render_band(runspaces, ba, frame);
    }
}

/// Word-wrap `text` so no line exceeds `max_cols` characters.
///
/// Breaks at whitespace boundaries. A word longer than `max_cols` is placed
/// on its own line and not split mid-character — hard-breaking within a word
/// would look worse than a slightly over-long line.
fn wrap_at(text: &str, max_cols: usize) -> Vec<String> {
    if text.len() <= max_cols {
        return vec![text.to_string()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= max_cols {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Returns `(synonym, canonical, suffix)` if the input starts with a known synonym trigger.
///
/// Only returns `Some` when the typed trigger differs from the canonical
/// (i.e. it is actually a synonym, not the canonical word itself).
/// `suffix` is the rest of the input after the trigger (e.g. `" python"` for `"get python"`).
fn find_synonym(input: &str) -> Option<(String, String, String)> {
    let words: Vec<&str> = input.split_whitespace().collect();
    for len in (1..=3.min(words.len())).rev() {
        if let Some((canonical, _kind, Some(synonym))) = detect_trigger(&words[..len]) {
            let suffix = if words.len() > len {
                format!(" {}", words[len..].join(" "))
            } else {
                String::new()
            };
            return Some((synonym, canonical, suffix));
        }
    }
    None
}

/// Match the first 1–3 typed words against known command / directive triggers.
///
/// Returns `(canonical_name, kind, synonym_to_animate)`.
/// `synonym_to_animate` is `Some` when the typed word(s) differ from the canonical name.
fn detect_trigger(words: &[&str]) -> Option<(String, CmdKind, Option<String>)> {
    let syn = |typed: &str, canonical: &str| -> Option<String> {
        if typed != canonical { Some(typed.to_string()) } else { None }
    };
    let msyn = |typed: &[&str], canonical: &str| -> Option<String> {
        let joined = typed.join(" ");
        if joined != canonical { Some(joined) } else { None }
    };

    match words {
        // ── Commands (structural / meta — get ! prefix) ───────────────────────
        ["help"]      => Some(("help".into(),     CmdKind::Command, None)),
        ["?"]         => Some(("help".into(),     CmdKind::Command, Some("?".into()))),
        ["commands"]  => Some(("help".into(),     CmdKind::Command, Some("commands".into()))),
        ["list"]      => Some(("list".into(),     CmdKind::Command, None)),
        ["ls"]        => Some(("list".into(),     CmdKind::Command, Some("ls".into()))),
        ["history"]   => Some(("history".into(),  CmdKind::Command, None)),
        ["log"]       => Some(("history".into(),  CmdKind::Command, Some("log".into()))),
        ["scan"]      => Some(("scan".into(),     CmdKind::Command, None)),
        ["expand"]    => Some(("expand".into(),   CmdKind::Command, None)),
        ["verify"]    => Some(("verify".into(),   CmdKind::Command, None)),
        ["check"]     => Some(("verify".into(),   CmdKind::Command, Some("check".into()))),
        ["validate"]  => Some(("verify".into(),   CmdKind::Command, Some("validate".into()))),
        ["use"]       => Some(("use".into(),      CmdKind::Command, None)),
        ["activate"]  => Some(("use".into(),      CmdKind::Command, Some("activate".into()))),
        ["pin"]       => Some(("use".into(),      CmdKind::Command, Some("pin".into()))),
        ["register"]  => Some(("register".into(), CmdKind::Command, None)),
        ["active"]    => Some(("active".into(),   CmdKind::Command, None)),
        ["ollama"]    => Some(("ollama".into(),   CmdKind::Command, None)),
        ["ext"]       => Some(("ext".into(),      CmdKind::Command, None)),
        ["clr"]       => Some(("clr".into(),      CmdKind::Command, None)),
        ["switch", "to"]       => Some(("use".into(),            CmdKind::Command, msyn(words, "use"))),
        ["update", "rulesets"] => Some(("update-rulesets".into(), CmdKind::Command, None)),

        // ── Directives (executive — no ! prefix) ─────────────────────────────
        ["install"]  => Some(("install".into(),   CmdKind::Directive, None)),
        ["get"]      => Some(("install".into(),   CmdKind::Directive, syn("get",   "install"))),
        ["grab"]     => Some(("install".into(),   CmdKind::Directive, syn("grab",  "install"))),
        ["fetch"]    => Some(("install".into(),   CmdKind::Directive, syn("fetch", "install"))),
        ["add"]      => Some(("install".into(),   CmdKind::Directive, syn("add",   "install"))),
        ["setup"]    => Some(("install".into(),   CmdKind::Directive, syn("setup", "install"))),
        ["set", "up"] => Some(("install".into(),  CmdKind::Directive, msyn(words, "install"))),

        ["uninstall"] => Some(("uninstall".into(), CmdKind::Directive, None)),
        ["remove"]    => Some(("uninstall".into(), CmdKind::Directive, syn("remove",   "uninstall"))),
        ["delete"]    => Some(("uninstall".into(), CmdKind::Directive, syn("delete",   "uninstall"))),
        ["drop"]      => Some(("uninstall".into(), CmdKind::Directive, syn("drop",     "uninstall"))),
        ["get", "rid"] | ["get", "rid", "of"] =>
            Some(("uninstall".into(), CmdKind::Directive, msyn(words, "uninstall"))),
        ["take", "out"] =>
            Some(("uninstall".into(), CmdKind::Directive, msyn(words, "uninstall"))),

        ["update"]    => Some(("update".into(),    CmdKind::Directive, None)),
        ["upgrade"]   => Some(("update".into(),    CmdKind::Directive, syn("upgrade",  "update"))),
        ["refresh"]   => Some(("update".into(),    CmdKind::Directive, syn("refresh",  "update"))),
        ["update", "all"] => Some(("update all".into(), CmdKind::Directive, None)),

        ["rollback"]  => Some(("rollback".into(),  CmdKind::Directive, None)),
        ["revert"]    => Some(("rollback".into(),  CmdKind::Directive, syn("revert",   "rollback"))),
        ["undo"]      => Some(("rollback".into(),  CmdKind::Directive, syn("undo",     "rollback"))),
        ["downgrade"] => Some(("rollback".into(),  CmdKind::Directive, syn("downgrade","rollback"))),
        ["previous"]  => Some(("rollback".into(),  CmdKind::Directive, syn("previous", "rollback"))),
        ["go", "back"] => Some(("rollback".into(), CmdKind::Directive, msyn(words, "rollback"))),

        ["search"]    => Some(("search".into(),    CmdKind::Directive, None)),
        ["find"]      => Some(("search".into(),    CmdKind::Directive, syn("find",   "search"))),
        ["browse"]    => Some(("search".into(),    CmdKind::Directive, syn("browse", "search"))),
        ["look", "for"] => Some(("search".into(),  CmdKind::Directive, msyn(words, "search"))),

        ["info"]      => Some(("info".into(),      CmdKind::Directive, None)),
        ["about"]     => Some(("info".into(),      CmdKind::Directive, syn("about",    "info"))),
        ["show"]      => Some(("info".into(),      CmdKind::Directive, syn("show",     "info"))),
        ["describe"]  => Some(("info".into(),      CmdKind::Directive, syn("describe", "info"))),
        ["tell", "me", "about"] | ["show", "me"] | ["what", "about"] =>
            Some(("info".into(), CmdKind::Directive, msyn(words, "info"))),

        _ => None,
    }
}

/// Classify a submitted command string for history colouring.
fn classify_kind(cmd: &str) -> CmdKind {
    // Any input starting with ! is a structural command (blue).
    if cmd.starts_with('!') {
        return CmdKind::Command;
    }
    // Also classify by first word against detect_trigger so that commands
    // submitted from split mode (! already stripped) colour correctly in history.
    let words: Vec<&str> = cmd.split_whitespace().collect();
    if !words.is_empty() {
        if let Some((_, CmdKind::Command, _)) = detect_trigger(&words[..1]) {
            return CmdKind::Command;
        }
    }
    use lodge_brain::Command as C;
    let intent = lodge_brain::intent::resolve_deterministic(cmd);
    match intent.command {
        C::Help | C::List | C::History | C::Verify
        | C::UpdateRulesets | C::Scan | C::Expand | C::Use => CmdKind::Command,
        C::Install | C::Uninstall | C::Update | C::UpdateAll
        | C::Search | C::Info | C::Rollback => CmdKind::Directive,
        _ => CmdKind::None,
    }
}

/// Returns the ghost autocomplete suffix for `!` commands.
///
/// When the user has typed `!<partial>` with no space, and exactly one known
/// command starts with `<partial>`, returns the remaining characters.
/// Extension IDs from the registry are included alongside built-in commands.
fn ghost_completion(input: &str, ext_ids: &[String]) -> Option<String> {
    let partial = input.strip_prefix('!')?;
    if partial.is_empty() || partial.contains(' ') {
        return None;
    }
    const BUILTIN: &[&str] = &[
        "help", "list", "history", "scan", "expand",
        "verify", "use", "register", "active", "ollama", "ext", "clr",
    ];
    let mut all: Vec<&str> = BUILTIN.to_vec();
    let ext_refs: Vec<&str> = ext_ids.iter().map(String::as_str).collect();
    all.extend_from_slice(&ext_refs);

    let matches: Vec<&str> = all.iter()
        .copied()
        .filter(|c| c.starts_with(partial) && *c != partial)
        .collect();
    if matches.len() == 1 {
        Some(matches[0][partial.len()..].to_string())
    } else {
        None
    }
}

/// Parse a mixed-separator path list for `!register`.
///
/// Separators: `,`, `;`, or whitespace. Paths with internal spaces must be quoted.
fn parse_paths(args: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in args.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' | ';' if !in_quotes => {
                let p = current.trim().to_string();
                if !p.is_empty() { paths.push(p); }
                current.clear();
            }
            ' ' if !in_quotes => {
                let p = current.trim().to_string();
                if !p.is_empty() { paths.push(p); }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let p = current.trim().to_string();
    if !p.is_empty() { paths.push(p); }
    paths
}

/// Map a help topic keyword to a card index.
fn help_page_for_topic(topic: &str) -> usize {
    match topic.trim().to_lowercase().as_str() {
        "ai" | "key" | "provider" | "ollama" | "gemini" | "claude" => 5,
        "band" | "active" | "scan" | "expand" | "register" | "commands" => 4,
        "tools" | "dev" | "developer" => 2,
        "machine" | "system" | "hardware" | "specs" => 3,
        "discover" | "search" | "find" | "inspect" | "info" => 1,
        "extensions" | "ext" | "extension" | "plugins" => 6,
        "install" | "manage" | "update" | "rollback" | "uninstall" => 0,
        _ => 0,
    }
}

/// If the user right-clicked on a `✦` suggestion line in the history area,
/// return the suggestion text (everything after `✦ `), otherwise `None`.
fn pick_suggestion(
    mouse_y: u16,
    term_height: u16,
    history: &[(String, String)],
    history_scroll: u16,
    split_mode: bool,
) -> Option<String> {
    // Reproduce render_bar geometry:
    //   Row 0:         header
    //   Row 1:         top divider
    //   Rows 2..N-2-ir: history (ir = input_rows)
    //   Row N-2-ir+1:  mid divider
    //   Rows N-ir..N-1: input (1 or 2 rows)
    //   Row N-1:       bottom divider
    let input_rows = if split_mode { 2u16 } else { 1u16 };
    let hist_y = 2u16;
    let hist_height = term_height.saturating_sub(4 + input_rows);

    if mouse_y < hist_y || mouse_y >= hist_y + hist_height {
        return None;
    }

    let flat = build_flat_history_text(history);
    let history_height = flat.len() as u16;

    let line_idx: usize = if history_height <= hist_height {
        // Bottom-aligned: gap at the top of the history area.
        let start_y = hist_y + hist_height.saturating_sub(history_height);
        if mouse_y < start_y {
            return None;
        }
        (mouse_y - start_y) as usize
    } else {
        // Overflowing: scrolled view.
        let base = history_height - hist_height;
        let offset = base.saturating_sub(history_scroll);
        (mouse_y - hist_y + offset) as usize
    };

    flat.get(line_idx).and_then(|line| {
        line.trim_start()
            .strip_prefix('✦')
            .map(|rest| rest.trim().to_string())
    })
}

/// Expand `%VAR%` (Windows) and `$VAR` (Unix) tokens in a path string.
fn expand_env_path(path: &str) -> String {
    let mut s = path.to_string();
    // Expand %VAR% patterns
    let mut i = 0;
    while i < s.len() {
        if let Some(start) = s[i..].find('%') {
            let abs = i + start;
            if let Some(len) = s[abs + 1..].find('%') {
                let name = s[abs + 1..abs + 1 + len].to_string();
                if !name.is_empty() {
                    if let Ok(val) = std::env::var(&name) {
                        s = format!("{}{}{}", &s[..abs], val, &s[abs + 1 + len + 1..]);
                        i = abs + val.len();
                        continue;
                    }
                }
                i = abs + 1 + len + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    s
}

/// Ollama RAM requirements for common models (GB).
const OLLAMA_MODEL_RAM: &[(&str, f32)] = &[
    ("llama3.2:1b",      1.3),
    ("llama3.2:3b",      2.0),
    ("llama3.1:8b",      5.0),
    ("llama3.1:70b",    40.0),
    ("phi3:mini",        2.3),
    ("phi3:medium",      8.0),
    ("mistral:7b",       4.1),
    ("gemma2:2b",        1.6),
    ("gemma2:9b",        5.5),
    ("qwen2.5:7b",       4.7),
    ("deepseek-r1:7b",   4.7),
];

/// Renders a Lodge-style pre-launch card for an extension.
/// Blocks until the user presses Enter (proceed) or Esc (cancel).
/// Returns `Ok(true)` = proceed, `Ok(false)` = cancel.
fn show_extension_card(
    entry: &crate::engine::extensions::RegistryEntry,
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<bool> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};
    use ratatui::{
        layout::{Alignment, Constraint, Direction, Layout, Margin},
        style::{Modifier, Style, Stylize},
        text::{Line, Span},
        widgets::{Block, BorderType, Borders, Paragraph},
    };

    // Drain any buffered key events — specifically the Enter that submitted
    // the extension command must not be read as a confirmation in the card.
    while event::poll(std::time::Duration::ZERO)? {
        let _ = event::read();
    }

    loop {
        terminal.draw(|f| {
            let area = f.area();

            // Same geometry as flashcard.rs — centred fixed-width card.
            let card_w = area.width.min(58);
            let card_h = area.height.min(22);
            let h_pad = (area.width.saturating_sub(card_w)) / 2;
            let v_pad = (area.height.saturating_sub(card_h)) / 2;

            let card_area = ratatui::layout::Rect {
                x: h_pad,
                y: v_pad,
                width: card_w,
                height: card_h,
            };

            // Bordered block with SURFACE fill — matches install flashcard exactly.
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(palette::BORDER))
                .style(Style::default().bg(palette::SURFACE));
            f.render_widget(block, card_area);

            let inner = card_area.inner(Margin {
                horizontal: 2,
                vertical: 1,
            });

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // name + version
                    Constraint::Length(1), // type badge right-aligned
                    Constraint::Length(1), // blank
                    Constraint::Length(1), // description
                    Constraint::Length(1), // blank
                    Constraint::Length(1), // divider
                    Constraint::Length(1), // blank
                    Constraint::Length(1), // command
                    Constraint::Length(1), // status
                    Constraint::Length(1), // blank
                    Constraint::Length(1), // divider
                    Constraint::Length(1), // blank
                    Constraint::Length(1), // actions
                    Constraint::Min(0),
                ])
                .split(inner);

            // Row 0: name + version — clip name so version always shows.
            let version_str = format!("v{}", entry.version);
            let name_max    = (inner.width as usize).saturating_sub(2 + version_str.len());
            let name_fitted = {
                let chars: Vec<char> = entry.name.chars().collect();
                if chars.len() <= name_max {
                    entry.name.clone()
                } else {
                    chars[..name_max.saturating_sub(1)].iter().collect::<String>() + "…"
                }
            };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        name_fitted,
                        Style::default().fg(palette::TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(version_str, Style::default().fg(palette::TEXT_DIM)),
                ])),
                rows[0],
            );

            // Row 1: "extension" right-aligned in accent — mirrors the type badge in flashcard
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("{:>width$}", "extension", width = inner.width as usize),
                    Style::default().fg(palette::ACCENT),
                )),
                rows[1],
            );

            // Row 3: description, truncated to card width
            let desc: String = {
                let chars: Vec<char> = entry.description.chars().collect();
                let max = inner.width as usize;
                if chars.len() <= max {
                    entry.description.clone()
                } else {
                    let cut: String = chars[..max.saturating_sub(1)].iter().collect();
                    format!("{cut}…")
                }
            };
            f.render_widget(
                Paragraph::new(Span::styled(desc, Style::default().fg(palette::TEXT_DIM))),
                rows[3],
            );

            // Dividers (rows 5 and 10)
            let divider = "─".repeat(inner.width as usize);
            for &row_idx in &[5usize, 10] {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        &divider,
                        Style::default().fg(palette::BORDER),
                    )),
                    rows[row_idx],
                );
            }

            let label_style = Style::default().fg(palette::TEXT_DIM);
            let value_style = Style::default().fg(palette::TEXT);

            // Row 7: command alias
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("{:<14}", "command"), label_style),
                    Span::styled(format!("!{}", entry.command_alias()), value_style),
                ])),
                rows[7],
            );

            // Row 8: status
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("{:<14}", "status"), label_style),
                    Span::styled(&entry.status, value_style),
                ])),
                rows[8],
            );

            // Row 12: actions — matches install flashcard key style exactly
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("[I]", Style::default().fg(palette::ACCENT).bold()),
                    Span::styled(" step in          ", Style::default().fg(palette::TEXT_DIM)),
                    Span::styled("[C]", Style::default().fg(palette::TEXT_DIM).bold()),
                    Span::styled(" stay here", Style::default().fg(palette::TEXT_DIM)),
                ]))
                .alignment(Alignment::Center),
                rows[12],
            );
        })?;

        // Blocking read — same pattern as flashcard::show().
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('i') | KeyCode::Char('I') | KeyCode::Enter => return Ok(true),
                KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc   => return Ok(false),
                _ => {}
            }
        }
    }
}

/// Handle `!ollama <subcommand>` — Ollama model management.
/// Suspend Lodge's TUI, run a Lodge extension binary as a subprocess, then
/// restore the TUI when the process exits.
///
/// Resolution order for the binary:
///   1. `extensions/<name>[.exe]` alongside the running binary (or `./extensions/` in dev)
///   2. `extensions/<name>/<name>[.exe]` (extracted zip layout)
///   3. Download via the registry if a `payload_url` is present
///
/// The TUI is fully suspended before the subprocess starts so the extension
/// can own the terminal. After the subprocess exits the TUI is restored.
/// `scan_path` is passed to the extension as `--path <scan_path> --lodge`.
/// It is passed as a single `OsStr` argument so paths with spaces are safe.
fn launch_extension(
    name: &str,
    scan_path: &str,
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    let ext_dir = crate::engine::extensions::extensions_dir();

    // Locate the binary.
    // Candidate 1/2: extensions/ sub-dir (production layout).
    // Candidate 3: alongside lodge.exe itself (dev/cargo-run layout where
    //   `cargo build` places both binaries in target/debug/).
    let bin_name = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let exe_sibling = std::env::current_exe().ok()
        .and_then(|p| p.parent().map(|d| d.join(&bin_name)));
    let mut candidates: Vec<std::path::PathBuf> = vec![
        ext_dir.join(&bin_name),
        ext_dir.join(name).join(&bin_name),
    ];
    if let Some(s) = exe_sibling { candidates.push(s); }

    let bin_path = candidates.iter().find(|p| p.exists()).cloned()
        .or_else(|| try_download_extension(name, &ext_dir).ok())
        .ok_or_else(|| anyhow::anyhow!(
            "extension '{name}' not found.\n  \
             install it by placing {bin_name} in the extensions/ directory,\n  \
             or wait for it to appear in the registry."
        ))?;

    // Suspend TUI.
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    )?;
    terminal.show_cursor()?;

    // Spawn extension — pass path and lodge flag as separate args so paths
    // with spaces are handled correctly (no split_whitespace fragmentation).
    let status = std::process::Command::new(&bin_path)
        .arg("--lodge")
        .arg("--path")
        .arg(scan_path)
        .status();

    // Restore TUI regardless of subprocess result.
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;
    terminal.clear()?;

    status?;
    Ok(())
}

/// Try to download an extension binary from the registry, extract it into
/// `dest_dir`, and return the path to the binary.
fn try_download_extension(name: &str, dest_dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let (entries, _online) = crate::engine::extensions::fetch_registry();
    let entry = entries.iter()
        .find(|e| e.id == name)
        .ok_or_else(|| anyhow::anyhow!("'{name}' not in registry"))?;

    if entry.payload_url.as_deref().map(str::is_empty).unwrap_or(true) {
        anyhow::bail!("no download URL for '{name}'");
    }

    // Download + verify.
    let (zip_path, _verified) = crate::engine::extensions::download_extension(entry, dest_dir)?;

    // Extract the zip.
    let zip_file = std::fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(zip_file)?;
    archive.extract(dest_dir)?;
    std::fs::remove_file(&zip_path).ok();

    // Find the binary after extraction.
    let bin_name = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let candidates = [
        dest_dir.join(&bin_name),
        dest_dir.join(name).join(&bin_name),
    ];
    candidates.iter().find(|p| p.exists()).cloned()
        .ok_or_else(|| anyhow::anyhow!("binary '{bin_name}' not found after extraction"))
}

fn handle_ollama_command(args: &str) -> String {
    let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
    let rest = rest.trim();

    match sub {
        "" | "status" => {
            // Show whether Ollama is reachable
            if lodge_brain::ai::ollama_reachable() {
                // List pulled models
                let out = std::process::Command::new("ollama")
                    .args(["list"])
                    .output();
                match out {
                    Ok(o) if o.status.success() => {
                        let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if text.is_empty() || (text.contains("NAME") && text.lines().count() <= 1) {
                            "Ollama is running. no models pulled yet.\n  use  !ollama install <model>  to pull one.".into()
                        } else {
                            format!("Ollama is running.\n{text}")
                        }
                    }
                    _ => "Ollama is running but  ollama list  failed.".into(),
                }
            } else {
                "Ollama is not running.\n  install from ollama.com or run:  winget install Ollama.Ollama".into()
            }
        }

        "models" | "list-models" => {
            let mut out = "suggested models  (RAM required):\n".to_string();
            for (model, gb) in OLLAMA_MODEL_RAM {
                out.push_str(&format!("  {model:<28} ~{gb:.1} GB\n"));
            }
            out.push_str("\n  use  !ollama install <model>  to pull one.");
            out
        }

        "list" => {
            let result = std::process::Command::new("ollama")
                .args(["list"])
                .output();
            match result {
                Ok(o) if o.status.success() => {
                    let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if text.is_empty() { "no models pulled.".into() } else { text }
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    format!("ollama list failed: {err}")
                }
                Err(e) => format!("couldn't run ollama: {e}"),
            }
        }

        "install" | "pull" => {
            if rest.is_empty() {
                return "usage: !ollama install <model>  (e.g. llama3.2:3b)\n  type  !ollama models  to see suggestions.".into();
            }
            let model = rest.to_string();

            // RAM check
            let mut warning = String::new();
            let ram_result = lodge_brain::scout::dispatch("ram_usage", &Default::default());
            if let Some(probe) = ram_result {
                let raw = probe.value.or(probe.raw).unwrap_or_default();
                if let Some(pct) = parse_ram_pct(&raw) {
                    if pct > 40.0 {
                        warning = format!(
                            "! RAM usage is currently {pct:.0}% — model performance may be affected.\n"
                        );
                    }
                }
                let req_gb = OLLAMA_MODEL_RAM.iter()
                    .find(|(m, _)| *m == model)
                    .map(|(_, gb)| *gb);
                if let Some(req) = req_gb {
                    let total_gb = parse_total_gb(&raw).unwrap_or(0.0);
                    let free_gb = total_gb * (1.0 - parse_ram_pct(&raw).unwrap_or(0.0) / 100.0);
                    if free_gb < req {
                        warning.push_str(&format!(
                            "! {model} needs ~{req:.1} GB, only ~{free_gb:.1} GB available.\n"
                        ));
                    }
                }
            }

            // Run ollama pull
            let result = std::process::Command::new("ollama")
                .args(["pull", &model])
                .output();
            match result {
                Ok(o) if o.status.success() => {
                    format!("{warning}{model} pulled.")
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    format!("{warning}pull failed: {err}")
                }
                Err(e) => format!("couldn't run ollama: {e}\n  is Ollama installed?"),
            }
        }

        "remove" | "rm" | "uninstall" => {
            if rest.is_empty() {
                return "usage: !ollama remove <model>".into();
            }
            let model = rest.to_string();
            let result = std::process::Command::new("ollama")
                .args(["rm", &model])
                .output();
            match result {
                Ok(o) if o.status.success() => format!("{model} removed."),
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    format!("remove failed: {err}")
                }
                Err(e) => format!("couldn't run ollama: {e}"),
            }
        }

        _ => "usage: !ollama  |  !ollama install <model>  |  !ollama remove <model>  |  !ollama list  |  !ollama models".into(),
    }
}

fn parse_ram_pct(raw: &str) -> Option<f32> {
    // Matches "X GB used / Y GB total  (Z% used)" or "Z% used"
    for part in raw.split_whitespace() {
        let part = part.trim_end_matches('%');
        if let Ok(v) = part.parse::<f32>() {
            if v > 0.0 && v <= 100.0 && raw.contains('%') {
                return Some(v);
            }
        }
    }
    None
}

fn parse_total_gb(raw: &str) -> Option<f32> {
    // "X GB used / Y GB total"
    if let Some(idx) = raw.find("total") {
        let before = &raw[..idx];
        let last_num = before.split_whitespace()
            .filter_map(|w| w.parse::<f32>().ok())
            .next_back()?;
        return Some(last_num);
    }
    None
}

/// List the contents of a path — compact summary by default, full stem listing with `with_files`.
///
/// Default (compact):
/// ```
///   path [N items — D dirs, F files]
///   dirs: src  tests  docs
///   .rs×412  .toml×23  .json×18  .md×15  [4 more types]
/// ```
fn probe_directory(path: &str, with_files: bool) -> String {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return format!("{path}: not found");
    }
    if p.is_file() {
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        return format!("{path}: file ({size}B)");
    }
    let entries = match std::fs::read_dir(p) {
        Err(e) => return format!("{path}: couldn't read ({e})"),
        Ok(e) => e,
    };

    let mut dirs: Vec<String> = Vec::new();
    // BTreeMap keeps extension groups in stable alphabetical order.
    let mut by_ext: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            dirs.push(name);
        } else {
            // Split into stem + extension; no-extension files go in the "" bucket.
            let (stem, ext) = match name.rfind('.') {
                Some(i) if i > 0 => (name[..i].to_string(), name[i + 1..].to_string()),
                _ => (name.clone(), String::new()),
            };
            by_ext.entry(ext).or_default().push(stem);
        }
    }

    dirs.sort_unstable();
    let file_count: usize = by_ext.values().map(|v| v.len()).sum();
    let total = dirs.len() + file_count;

    let mut out = format!(
        "{path} [{total} items — {} dirs, {file_count} files]",
        dirs.len()
    );

    // Dirs: always shown (usually few, rarely >20)
    if !dirs.is_empty() {
        let dir_list = if dirs.len() <= 12 {
            dirs.join("  ")
        } else {
            format!("{}  [{} more]", dirs[..12].join("  "), dirs.len() - 12)
        };
        out.push_str(&format!("\n  dirs: {dir_list}"));
    }

    let mut groups: Vec<(String, Vec<String>)> = by_ext.into_iter().collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.1.len()));

    if with_files {
        // Full stem listing — same as original behaviour
        for (ext, mut stems) in groups {
            stems.sort_unstable();
            let label = if ext.is_empty() {
                format!("  [no-ext] ({}): ", stems.len())
            } else {
                format!("  .{ext} ({}): ", stems.len())
            };
            out.push_str(&format!("\n{label}{}", stems.join("  ")));
        }
    } else {
        // Compact summary: type×count  type×count  [N more types]
        const SHOW_TYPES: usize = 6;
        let shown: Vec<String> = groups.iter().take(SHOW_TYPES).map(|(ext, v)| {
            if ext.is_empty() {
                format!("[no-ext]×{}", v.len())
            } else {
                format!(".{ext}×{}", v.len())
            }
        }).collect();
        let remaining = groups.len().saturating_sub(SHOW_TYPES);
        let mut type_line = shown.join("  ");
        if remaining > 0 {
            type_line.push_str(&format!("  [{remaining} more types]"));
        }
        if !type_line.is_empty() {
            out.push_str(&format!("\n  {type_line}"));
        }
        out.push_str("\n  (use  !register --files <path>  to list individual filenames)");
    }

    out
}

/// Build the flat plain-text line list that mirrors what `render_bar` produces
/// for the history area, used by `pick_suggestion` to map a mouse row to content.
fn build_flat_history_text(history: &[(String, String)]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (cmd, resp) in history {
        if !cmd.is_empty() {
            lines.push(format!("  > {cmd}"));
        }
        if !resp.is_empty() {
            for part in resp.split('\n') {
                for wrapped in wrap_at(part, 98) {
                    lines.push(format!("  {wrapped}"));
                }
            }
            lines.push(String::new());
        }
    }
    lines
}
