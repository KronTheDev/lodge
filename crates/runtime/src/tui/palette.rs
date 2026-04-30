use ratatui::style::Color;

// ── Lodge colour palette ──────────────────────────────────────────────────────
// Warm cabin aesthetic — aged timber, firelight, worn leather.
// All TUI code must reference these constants; never hardcode RGB values inline.

/// `#1c1510` — dark walnut. Primary terminal background.
#[allow(dead_code)]
pub const BG: Color = Color::Rgb(28, 21, 16);
/// `#26190f` — worn timber. Panel / card background.
pub const SURFACE: Color = Color::Rgb(38, 25, 15);
/// `#3d2b1a` — wood grain. Borders and dividers.
pub const BORDER: Color = Color::Rgb(61, 43, 26);
/// `#f0e6d3` — warm parchment. Primary readable text.
pub const TEXT: Color = Color::Rgb(240, 230, 211);
/// `#a08060` — faded ink. Muted labels, hints, secondary text.
pub const TEXT_DIM: Color = Color::Rgb(160, 128, 96);
/// `#c8813a` — ember orange. Primary interactive element, wordmark.
pub const ACCENT: Color = Color::Rgb(200, 129, 58);
/// `#7a9e6a` — pine green. Completions, confirmations, done steps.
pub const SUCCESS: Color = Color::Rgb(122, 158, 106);
/// `#b85c4a` — hearthstone red. Failures, hard stops.
pub const ERROR: Color = Color::Rgb(184, 92, 74);
/// `#c49a3a` — lantern amber. Cautions, soft warnings.
pub const WARNING: Color = Color::Rgb(196, 154, 58);
/// `#7a9ab0` — morning frost. Active steps, in-progress spinners.
pub const IN_PROGRESS: Color = Color::Rgb(122, 154, 176);
/// `#e8c98a` — candlelight. Focused element, cursor highlight.
pub const HIGHLIGHT: Color = Color::Rgb(232, 201, 138);
