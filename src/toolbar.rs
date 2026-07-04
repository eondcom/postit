use iced::widget::{button, container, row, text};
use iced::{Alignment, Border, Color, Element, Length};

use crate::app::Message;
use crate::colors::NoteColor;

/// The always-on-top color-swatch bar used to spawn new notes.
///
/// This is the fallback UI used only when the system tray icon could not be
/// registered (see `tray.rs`); it additionally exposes the "포스트잇 목록"
/// (☰) and "종료" (✕) actions that a working tray would otherwise offer
/// through its context menu.
pub fn view<'a>() -> Element<'a, Message> {
    let swatches = NoteColor::ALL.iter().fold(
        row![].spacing(8).align_y(Alignment::Center),
        |row, &color| row.push(swatch(color)),
    );

    let content = row![swatches, list_button(), quit_button()]
        .spacing(10)
        .align_y(Alignment::Center);

    container(content)
        .padding(6)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgb8(250, 250, 245).into()),
            border: Border {
                color: Color::from_rgb8(200, 200, 195),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn list_button<'a>() -> Element<'a, Message> {
    button(text("☰").size(14))
        .padding(4)
        .on_press(Message::ToggleList)
        .style(plain_button_style)
        .into()
}

fn quit_button<'a>() -> Element<'a, Message> {
    button(text("✕").size(14))
        .padding(4)
        .on_press(Message::Quit)
        .style(plain_button_style)
        .into()
}

fn plain_button_style(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: Color::from_rgb8(62, 39, 35),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn swatch<'a>(color: NoteColor) -> Element<'a, Message> {
    button(iced::widget::space::horizontal().width(20).height(20))
        .padding(2)
        .on_press(Message::CreateNote(color))
        .style(move |_theme, _status| button::Style {
            background: Some(color.bg().into()),
            border: Border {
                color: color.border(),
                width: 2.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}
