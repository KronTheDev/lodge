//! Lodge colour palette — duplicated from the runtime crate so clean-cabin
//! stays independent without importing the full lodge binary.

use ratatui::style::Color;

pub const BG: Color = Color::Rgb(0x1c, 0x15, 0x10);
pub const SURFACE: Color = Color::Rgb(0x26, 0x19, 0x0f);
pub const BORDER: Color = Color::Rgb(0x3d, 0x2b, 0x1a);
pub const TEXT: Color = Color::Rgb(0xf0, 0xe6, 0xd3);
pub const DIM: Color = Color::Rgb(0xa0, 0x80, 0x60);
pub const ACCENT: Color = Color::Rgb(0xc8, 0x81, 0x3a);
#[allow(dead_code)]
pub const SUCCESS: Color = Color::Rgb(0x7a, 0x9e, 0x6a);
pub const ERROR: Color = Color::Rgb(0xb8, 0x5c, 0x4a);
pub const WARN: Color = Color::Rgb(0xc4, 0x9a, 0x3a);
#[allow(dead_code)]
pub const FROST: Color = Color::Rgb(0x7a, 0x9a, 0xb0);
pub const CANDLE: Color = Color::Rgb(0xe8, 0xc9, 0x8a);
