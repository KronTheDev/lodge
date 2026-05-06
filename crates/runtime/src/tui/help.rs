// Card-based help overlay.
//
// Renders a centred card over whatever is currently on screen.
// Four cards, navigated with Left/Right arrows or Tab.
// Q or Esc closes the overlay and returns to the command bar.

use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use super::palette;

/// Total number of help cards.
pub const TOTAL_CARDS: usize = 6;

/// Renders the help card overlay centred in the terminal.
///
/// `page` is 0-indexed; wraps at [`TOTAL_CARDS`].
pub fn render(page: usize, frame: &mut Frame) {
    let area = frame.area();

    let card_w = 78u16.min(area.width.saturating_sub(2));
    let card_h = 33u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(card_w)) / 2;
    let y = area.y + (area.height.saturating_sub(card_h)) / 2;
    let card_area = Rect { x, y, width: card_w, height: card_h };

    // Clear the region so the bar underneath doesn't bleed through.
    frame.render_widget(Clear, card_area);

    let title_line = Line::from(vec![
        Span::raw(" "),
        Span::styled(card_title(page), Style::default().fg(palette::ACCENT)),
        Span::raw(" "),
    ]);

    let block = Block::default()
        .title(title_line)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(palette::BORDER))
        .style(Style::default().bg(palette::SURFACE));

    let inner = block.inner(card_area);
    frame.render_widget(block, card_area);

    // Reserve the last 2 rows for separator + navigation.
    let content_h = inner.height.saturating_sub(2);

    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: content_h,
    };
    let sep_area = Rect {
        x: inner.x,
        y: inner.y + content_h,
        width: inner.width,
        height: 1,
    };
    let nav_area = Rect {
        x: inner.x,
        y: inner.y + content_h + 1,
        width: inner.width,
        height: 1,
    };

    frame.render_widget(Paragraph::new(card_content(page)), content_area);

    let sep = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            sep,
            Style::default().fg(palette::BORDER),
        ))),
        sep_area,
    );

    frame.render_widget(
        Paragraph::new(nav_bar(page)).alignment(Alignment::Center),
        nav_area,
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn card_title(page: usize) -> &'static str {
    match page {
        0 => "install & manage",
        1 => "discover & inspect",
        2 => "ask — developer tools",
        3 => "ask — your machine",
        4 => "band & commands",
        5 => "ai settings",
        _ => "help",
    }
}

fn nav_bar(page: usize) -> Line<'static> {
    let mut spans: Vec<Span<'static>> =
        vec![Span::styled("[←][→]  ", Style::default().fg(palette::TEXT_DIM))];

    for i in 0..TOTAL_CARDS {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        if i == page {
            spans.push(Span::styled("●", Style::default().fg(palette::ACCENT)));
        } else {
            spans.push(Span::styled("○", Style::default().fg(palette::TEXT_DIM)));
        }
    }

    spans.push(Span::styled("  [Q] close", Style::default().fg(palette::TEXT_DIM)));
    Line::from(spans)
}

/// A command row: command in ACCENT, description in TEXT_DIM.
fn row(cmd: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(cmd, Style::default().fg(palette::ACCENT)),
        Span::styled(desc, Style::default().fg(palette::TEXT_DIM)),
    ])
}

/// A query row: natural-language example in TEXT, probe note in TEXT_DIM.
fn qrow(question: &'static str, note: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(question, Style::default().fg(palette::TEXT)),
        Span::styled(note, Style::default().fg(palette::TEXT_DIM)),
    ])
}

/// A dimmed section header / footnote.
fn head(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(palette::TEXT_DIM)))
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn card_content(page: usize) -> Vec<Line<'static>> {
    match page {
        0 => card_install(),
        1 => card_discover(),
        2 => card_dev_tools(),
        3 => card_machine(),
        4 => card_band(),
        5 => card_ai(),
        _ => vec![],
    }
}

// ── Card 0 — install & manage ─────────────────────────────────────────────────

fn card_install() -> Vec<Line<'static>> {
    vec![
        blank(),
        row("install <path>            ", "install a package from a local path"),
        row("install <id>              ", "install the latest version from the feed"),
        row("install <id>@<version>    ", "install a specific version"),
        blank(),
        row("uninstall <id>            ", "remove an installed package"),
        row("update <id>               ", "update a package to the latest version"),
        row("update all                ", "update every installed package"),
        row("rollback <id>             ", "revert to the previous version"),
        blank(),
        row("use <id>@<version>        ", "switch which version the shim resolves to"),
        row("verify <id>               ", "check that all installed files are intact"),
        row("history                   ", "show the installation log"),
        blank(),
        head("  shims    %LOCALAPPDATA%\\Programs\\lodge\\shims\\"),
        head("  receipts  %LOCALAPPDATA%\\lodge\\receipts\\"),
        blank(),
        head("  every install writes a signed receipt used for clean uninstall"),
    ]
}

// ── Card 1 — discover & inspect ──────────────────────────────────────────────

fn card_discover() -> Vec<Line<'static>> {
    vec![
        blank(),
        row("list                      ", "show all installed packages"),
        row("search                    ", "list everything in the local feed"),
        row("search <query>            ", "find a package by name or keyword"),
        row("info <id>                 ", "show details for an installed package"),
        blank(),
        head("  natural language aliases"),
        blank(),
        qrow("what's installed?         ", "→ list"),
        qrow("what do I have?           ", "→ list"),
        qrow("find <query>              ", "→ search"),
        qrow("what is <id>?             ", "→ info"),
        qrow("tell me about <id>        ", "→ info"),
        qrow("show me <id>              ", "→ info"),
        qrow("what about <id>?          ", "→ info"),
        blank(),
        head("  the local feed is scanned from the directory lodge was run from"),
    ]
}

// ── Card 2 — ask: developer tools ────────────────────────────────────────────

fn card_dev_tools() -> Vec<Line<'static>> {
    vec![
        blank(),
        head("  ask naturally — any phrasing that implies the question works"),
        blank(),
        qrow("do I have node installed?        ", "node version"),
        qrow("what Python version am I on?     ", "python version"),
        qrow("is npm installed?                ", "npm version"),
        qrow("what git version?                ", "git version"),
        qrow("do I have Java?                  ", "java version"),
        qrow("is Go installed?                 ", "go / golang version"),
        qrow("do I have Ruby?                  ", "ruby version"),
        qrow("is Docker running?               ", "docker version"),
        qrow("do I have PHP?                   ", "php version"),
        blank(),
        qrow("is winget available?             ", "winget version"),
        qrow("is Scoop installed?              ", "scoop version"),
        blank(),
        qrow("is <app> installed?              ", "generic app check"),
    ]
}

// ── Card 3 — ask: your machine ────────────────────────────────────────────────

fn card_machine() -> Vec<Line<'static>> {
    vec![
        blank(),
        qrow("how much RAM do I have?          ", "total + free memory"),
        qrow("how much space on C:?            ", "free disk space on a drive"),
        qrow("how much space on all drives?    ", "all drives at once"),
        qrow("what CPU do I have?              ", "processor model and core count"),
        qrow("do I have a GPU?                 ", "graphics adapter info"),
        qrow("how's my battery?                ", "charge level and status"),
        qrow("what OS build am I on?           ", "Windows build number"),
        qrow("how long has this been running?  ", "system uptime"),
        qrow("what's my hostname?              ", "machine name"),
        qrow("who am I logged in as?           ", "current username"),
        qrow("what's my local IP?              ", "LAN address"),
        blank(),
        qrow("is port 8080 free?               ", "port availability"),
        qrow("is my execution policy a problem?", "PowerShell execution policy"),
        qrow("is <process> running?            ", "process list check"),
        qrow("is <service> running?            ", "Windows service / systemd unit"),
    ]
}

// ── Card 4 — band & commands ──────────────────────────────────────────────────

fn card_band() -> Vec<Line<'static>> {
    vec![
        blank(),
        head("  live band  (visible when terminal ≥ 100 columns wide)"),
        blank(),
        row("!active                   ", "pin last probe result to the live band"),
        row("!active <probe> [args]    ", "pin a named probe to the band (live refresh)"),
        row("!active clear             ", "remove all runspaces from the band"),
        blank(),
        head("  up to 5 runspaces — each refreshes every 12 seconds"),
        head("  progress bars fill automatically for RAM, disk, CPU load"),
        head("  ◐◑◒◓ spinner on runspaces with live probes attached"),
        blank(),
        head("  file context"),
        blank(),
        row("!register <path> [...]    ", "probe paths and store context for !expand"),
        row("!register                 ", "list currently registered paths"),
        blank(),
        head("  ai narration"),
        blank(),
        row("!scan                     ", "run a full system snapshot with AI narration"),
        row("!expand                   ", "ask AI to explain the last probe result"),
        row("!expand <question>        ", "ask a follow-up question about the last result"),
        blank(),
        head("  navigation"),
        blank(),
        head("  Tab         cycle through ✦ suggestions in history"),
        head("  hover ✦     highlights the suggestion under the cursor"),
        head("  click ✦     fills the input with that suggestion"),
    ]
}

// ── Card 5 — ai settings ──────────────────────────────────────────────────────

fn card_ai() -> Vec<Line<'static>> {
    vec![
        blank(),
        head("  provider resolution: saved config → env vars → Ollama → none"),
        blank(),
        head("  check / change provider"),
        blank(),
        row("!key                      ", "show active AI provider"),
        row("!key set <api-key>        ", "save a key (auto-detects Gemini / Claude)"),
        row("!key ollama               ", "use local Ollama"),
        row("!key clear                ", "remove saved config"),
        blank(),
        head("  all genai-compatible providers are supported:"),
        blank(),
        head("  Ollama    local · free · install from ollama.com"),
        head("  Gemini    aistudio.google.com · free tier"),
        head("  Claude    console.anthropic.com"),
        head("  OpenAI    platform.openai.com  · gpt-4o-mini"),
        head("  Groq      console.groq.com · free · fast"),
        head("  xAI       console.x.ai · Grok models"),
        head("  DeepSeek  platform.deepseek.com · very cheap"),
        head("  Cohere    dashboard.cohere.com · free tier"),
        blank(),
        head("  Ollama model management"),
        blank(),
        row("!ollama                   ", "show Ollama status and pulled models"),
        row("!ollama install <model>   ", "pull a model (checks RAM first)"),
        row("!ollama remove <model>    ", "remove a pulled model"),
        row("!ollama models            ", "list suggested models with RAM requirements"),
    ]
}
