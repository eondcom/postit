//! Lucide icons (ISC license — see `assets/icons/LICENSE`), embedded at
//! compile time and rendered as single-color ("symbolic") glyphs.
//!
//! Every `.svg` here uses `stroke="currentColor"`, but that's irrelevant to
//! how they're drawn: `icon()` below sets `svg::Style::color`, and iced's
//! tiny_skia renderer honors that by keeping the rasterized alpha but
//! replacing the RGB channel wholesale — so the source color is always
//! overridden by whatever `color` the caller passes.

use iced::widget::svg;
use iced::{Color, Element, Length};

pub const CHEVRON_DOWN: &[u8] = include_bytes!("../assets/icons/chevron-down.svg");
pub const CHEVRON_UP: &[u8] = include_bytes!("../assets/icons/chevron-up.svg");
pub const PIN: &[u8] = include_bytes!("../assets/icons/pin.svg");
pub const PIN_OFF: &[u8] = include_bytes!("../assets/icons/pin-off.svg");
pub const MONITOR: &[u8] = include_bytes!("../assets/icons/monitor.svg");
pub const LINK: &[u8] = include_bytes!("../assets/icons/link.svg");
pub const TRASH_2: &[u8] = include_bytes!("../assets/icons/trash-2.svg");
pub const X: &[u8] = include_bytes!("../assets/icons/x.svg");
pub const GRIP_VERTICAL: &[u8] = include_bytes!("../assets/icons/grip-vertical.svg");
pub const DOWNLOAD: &[u8] = include_bytes!("../assets/icons/download.svg");

/// Builds a single-color SVG icon element, `size` logical px square, tinted
/// `color`. See module docs for why the source SVG's own color is ignored.
pub fn icon<'a, Message: 'a>(
    bytes: &'static [u8],
    size: f32,
    color: Color,
) -> Element<'a, Message> {
    svg::Svg::new(svg::Handle::from_memory(bytes))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_theme, _status| svg::Style { color: Some(color) })
        .into()
}
