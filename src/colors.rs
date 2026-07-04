use serde::{Deserialize, Serialize};
use iced::Color;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteColor {
    Yellow,
    Pink,
    Blue,
    Green,
    Orange,
    Gray,
}

impl NoteColor {
    pub const ALL: [NoteColor; 6] = [
        NoteColor::Yellow,
        NoteColor::Pink,
        NoteColor::Blue,
        NoteColor::Green,
        NoteColor::Orange,
        NoteColor::Gray,
    ];

    pub fn bg(&self) -> Color {
        match self {
            NoteColor::Yellow => Color::from_rgb8(255, 241, 118),
            NoteColor::Pink => Color::from_rgb8(244, 143, 177),
            NoteColor::Blue => Color::from_rgb8(129, 212, 250),
            NoteColor::Green => Color::from_rgb8(165, 214, 167),
            NoteColor::Orange => Color::from_rgb8(255, 183, 77),
            NoteColor::Gray => Color::from_rgb8(224, 224, 224),
        }
    }

    pub fn border(&self) -> Color {
        match self {
            NoteColor::Yellow => Color::from_rgb8(249, 215, 28),
            NoteColor::Pink => Color::from_rgb8(236, 95, 143),
            NoteColor::Blue => Color::from_rgb8(79, 195, 247),
            NoteColor::Green => Color::from_rgb8(121, 203, 126),
            NoteColor::Orange => Color::from_rgb8(255, 160, 30),
            NoteColor::Gray => Color::from_rgb8(158, 158, 158),
        }
    }

    pub fn text(&self) -> Color {
        Color::from_rgb8(62, 39, 35)
    }
}
