use std::collections::HashMap;

use iced::widget::{button, column, container, row, scrollable, space, text, MouseArea};
use iced::{Alignment, Border, Color, Element, Length};

use crate::app::Message;
use crate::colors::NoteColor;
use crate::icons;
use crate::note::Note;

/// Color used for the list panel's icon buttons (grip, monitor, close,
/// import) — matches `list_button_style`'s text color, so the SVG icons read
/// the same brown as the panel's text labels.
const LIST_ICON_COLOR: Color = Color::from_rgb8(62, 39, 35);

/// Maximum characters shown for a note's text before it gets truncated with
/// an ellipsis.
const MAX_LABEL_CHARS: usize = 24;

/// Renders the "포스트잇 목록" panel: every note (including ones currently
/// hidden by the app-binding rules or parked off-screen), each with a
/// [가져오기] button to bring it back to a known on-screen position and a
/// [삭제] button to remove it for good.
pub fn view<'a>(notes: &'a HashMap<u64, Note>) -> Element<'a, Message> {
    let mut sorted: Vec<&Note> = notes.values().collect();
    sorted.sort_by_key(|note| note.id);

    let header = row![
        drag_grip(),
        text("포스트잇 목록").size(14),
        space::horizontal(),
        move_button(),
        close_button(),
    ]
    .align_y(Alignment::Center)
    .padding([4, 8]);

    let rows = sorted
        .into_iter()
        .fold(column![].spacing(2), |col, note| col.push(note_row(note)));

    let body = scrollable(rows.padding([0, 8])).height(Length::Fill);

    container(column![header, body].spacing(4))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(6)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgb8(250, 250, 245).into()),
            // The panel background is light regardless of the system theme,
            // so the inherited (theme) text color can be near-white and
            // unreadable — pin the text to the note text brown instead.
            text_color: Some(Color::from_rgb8(62, 39, 35)),
            border: Border {
                color: Color::from_rgb8(200, 200, 195),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn note_row<'a>(note: &'a Note) -> Element<'a, Message> {
    let label = label_for(note);

    row![
        color_chip(note.color),
        text(label).size(12).width(Length::Fill),
        import_button(note.id),
        list_button("삭제", Message::DeleteNote(note.id)),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .padding([3, 4])
    .into()
}

fn label_for(note: &Note) -> String {
    let trimmed = note.text.trim();
    if trimmed.is_empty() {
        return "(빈 메모)".to_string();
    }
    if trimmed.chars().count() > MAX_LABEL_CHARS {
        let truncated: String = trimmed.chars().take(MAX_LABEL_CHARS).collect();
        format!("{truncated}…")
    } else {
        trimmed.to_string()
    }
}

fn color_chip<'a>(color: NoteColor) -> Element<'a, Message> {
    container(space::horizontal().width(12).height(12))
        .style(move |_theme| container::Style {
            background: Some(color.bg().into()),
            border: Border {
                color: color.border(),
                width: 1.5,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// The "가져오기" button: a small download icon plus its text label, since a
/// bare download glyph alone would be too ambiguous a control to fire from a
/// dense list row (unlike the single-purpose icon-only buttons elsewhere).
fn import_button<'a>(note_id: u64) -> Element<'a, Message> {
    let content = row![
        icons::icon(icons::DOWNLOAD, 11.0, LIST_ICON_COLOR),
        text("가져오기").size(11),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    button(content)
        .padding([2, 6])
        .on_press(Message::ImportNote(note_id))
        .style(list_button_style)
        .into()
}

fn list_button<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    button(text(label).size(11))
        .padding([2, 6])
        .on_press(message)
        .style(list_button_style)
        .into()
}

/// Drag handle at the left edge of the header, symmetric to `note_view`'s
/// note grip: pressing it starts a free-form drag of the whole panel. The
/// actual movement is tracked globally in `app.rs` via `event::listen_with`,
/// same rationale as the note grip (the cursor quickly outruns this narrow
/// strip during a fast drag).
fn drag_grip<'a>() -> Element<'a, Message> {
    // Fixed height: `Length::Fill` here makes the whole header row balloon
    // vertically (the row stretches to the tallest child's fill request).
    let icon = icons::icon(icons::GRIP_VERTICAL, 12.0, Color::from_rgb8(120, 108, 100));
    let handle = container(icon)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(22.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    MouseArea::new(handle)
        .interaction(iced::mouse::Interaction::Grab)
        .on_press(Message::ListDragStart)
        .into()
}

fn move_button<'a>() -> Element<'a, Message> {
    button(icons::icon(icons::MONITOR, 12.0, LIST_ICON_COLOR))
        .padding(4)
        .on_press(Message::MoveListToNextOutput)
        .style(list_button_style)
        .into()
}

fn close_button<'a>() -> Element<'a, Message> {
    button(icons::icon(icons::X, 12.0, LIST_ICON_COLOR))
        .padding(4)
        .on_press(Message::ToggleList)
        .style(list_button_style)
        .into()
}

fn list_button_style(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Color::from_rgb8(238, 238, 232).into()),
        text_color: Color::from_rgb8(62, 39, 35),
        border: Border {
            color: Color::from_rgb8(200, 200, 195),
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}
