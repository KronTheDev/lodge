use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::palette;

mod art {
    include!("splash_art.rs");
}

/// ASCII wordmark — rendered in accent colour, centred.
const WORDMARK: &str = "\
 ██╗      ██████╗ ██████╗  ██████╗ ███████╗\n\
 ██║     ██╔═══██╗██╔══██╗██╔════╝ ██╔════╝\n\
 ██║     ██║   ██║██║  ██║██║  ███╗█████╗  \n\
 ██║     ██║   ██║██║  ██║██║   ██║██╔══╝  \n\
 ███████╗╚██████╔╝██████╔╝╚██████╔╝███████╗\n\
 ╚══════╝ ╚═════╝ ╚═════╝  ╚═════╝ ╚══════╝";

const VERSION_LINE: &str = concat!(
    "v",
    env!("CARGO_PKG_VERSION"),
    "  ·  a place for everything"
);

/// Renders the Lodge splash screen into `frame`.
///
/// Layout (vertically centred):
/// 1. Cabin bracket art — 21 rows, ANSI truecolor
/// 2. Blank line
/// 3. ASCII wordmark — 6 rows, accent colour
/// 4. Version / tagline line
pub fn render(frame: &mut Frame) {
    let area = frame.area();

    let narrow = area.width < 82;

    let art_height = 21u16;
    let wordmark_height = 6u16;
    let total_height = art_height + 1 + wordmark_height + 1;
    let v_pad = area.height.saturating_sub(total_height) / 2;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(v_pad),
            Constraint::Length(art_height),
            Constraint::Length(1),
            Constraint::Length(wordmark_height),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    // Cabin art — convert ANSI escape sequences to ratatui spans
    if !narrow {
        let art_text = art::CABIN_ART.as_bytes().into_text().unwrap_or_default();
        let art_widget = Paragraph::new(art_text).alignment(Alignment::Center);
        frame.render_widget(art_widget, chunks[1]);
    }

    // Wordmark
    let wordmark_style = Style::default().fg(palette::ACCENT);
    if narrow {
        let simple =
            Paragraph::new(Span::styled("lodge", wordmark_style)).alignment(Alignment::Center);
        frame.render_widget(simple, chunks[3]);
    } else {
        let wordmark_widget = Paragraph::new(
            WORDMARK
                .lines()
                .map(|l| Line::from(Span::styled(l, wordmark_style)))
                .collect::<Vec<_>>(),
        )
        .alignment(Alignment::Center);
        frame.render_widget(wordmark_widget, chunks[3]);
    }

    // Version / tagline
    let version_widget = Paragraph::new(Span::styled(
        VERSION_LINE,
        Style::default().fg(palette::TEXT_DIM),
    ))
    .alignment(Alignment::Center);
    frame.render_widget(version_widget, chunks[4]);
}
