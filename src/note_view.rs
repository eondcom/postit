use iced::widget::{button, column, container, row, text, text_input, MouseArea};
use iced::{Alignment, Background, Border, Color, Element, Length};

use crate::app::Message;
use crate::colors::NoteColor;
use crate::note::Note;
use crate::settings::{AppSettings, SizePreset};

/// Note text/border colors keep at least this much alpha regardless of the
/// global opacity setting, so text stays legible even at low opacity (per
/// the "투명도 60%여도 글자는 잘 보이게" requirement).
const MIN_TEXT_ALPHA: f32 = 0.85;

/// Applies `settings.opacity_alpha()` to a color's existing alpha channel
/// (background/border use this directly).
fn with_opacity(color: Color, settings: &AppSettings) -> Color {
    Color {
        a: color.a * settings.opacity_alpha(),
        ..color
    }
}

/// Same as `with_opacity`, but floors the result at `MIN_TEXT_ALPHA` — used
/// for the note's text color so it stays readable at low opacity.
fn text_alpha(color: Color, settings: &AppSettings) -> Color {
    Color {
        a: (color.a * settings.opacity_alpha()).max(MIN_TEXT_ALPHA),
        ..color
    }
}

/// Renders a single note surface: the collapsed one-line row, plus (when
/// `expanded`) a second row with the color/pin/delete controls.
pub fn view<'a>(note: &'a Note, expanded: bool, settings: &AppSettings) -> Element<'a, Message> {
    let preset = settings.size_preset;
    let main_row = row![
        grip(note.id, note.color, preset),
        input(note, preset, settings),
        menu_button(note.id, expanded, preset),
        resize_handle(note.id, note.color, preset)
    ]
    .align_y(Alignment::Center)
    .spacing(0);

    let content: Element<'a, Message> = if expanded {
        column![main_row, options_row(note, preset)].into()
    } else {
        main_row.into()
    };

    let bg = with_opacity(note.color.bg(), settings);
    let border_color = with_opacity(note.color.border(), settings);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// The drag handle on the left edge of the note (width per
/// `SizePreset::grip_width`). Pressing it starts a drag; the actual movement
/// is tracked globally in `app.rs` via `event::listen_with`, since the
/// cursor quickly leaves this narrow strip during a fast drag and
/// per-widget hover-only callbacks would miss it.
fn grip<'a>(note_id: u64, color: NoteColor, preset: SizePreset) -> Element<'a, Message> {
    let handle = container(text("⣿").size(preset.grip_icon_text_size()).color(color.border()))
        .width(Length::Fixed(preset.grip_width()))
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    MouseArea::new(handle)
        .interaction(iced::mouse::Interaction::Grab)
        .on_press(Message::DragStart(note_id))
        .into()
}

/// The resize handle on the right edge of the note (width per
/// `SizePreset::resize_handle_width`), symmetric to `grip` above. Pressing
/// it starts a width resize; the actual resizing is tracked globally in
/// `app.rs` via `event::listen_with`, same rationale as the drag grip (the
/// cursor quickly outruns this narrow strip).
fn resize_handle<'a>(note_id: u64, color: NoteColor, preset: SizePreset) -> Element<'a, Message> {
    let handle = container(iced::widget::space::vertical())
        .width(Length::Fixed(preset.resize_handle_width()))
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(darken(color.border(), 0.75))),
            ..Default::default()
        });

    MouseArea::new(handle)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .on_press(Message::ResizeStart(note_id))
        .into()
}

/// Scales a color's RGB channels by `factor` (< 1.0 darkens), leaving alpha
/// untouched. Used to render the resize handle a shade darker than the
/// note's border color, per plan 9.2.
fn darken(color: Color, factor: f32) -> Color {
    Color {
        r: color.r * factor,
        g: color.g * factor,
        b: color.b * factor,
        a: color.a,
    }
}

fn input<'a>(note: &'a Note, preset: SizePreset, settings: &AppSettings) -> Element<'a, Message> {
    let note_id = note.id;
    let text_color = text_alpha(note.color.text(), settings);

    text_input("", &note.text)
        .on_input(move |value| Message::TextChanged(note_id, value))
        .padding(preset.input_padding())
        .size(preset.text_size())
        .style(move |theme, status| {
            let base = text_input::default(theme, status);
            text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                value: text_color,
                ..base
            }
        })
        .width(Length::Fill)
        .id(format!("postit-input-{}", note_id))
        .into()
}

fn menu_button<'a>(note_id: u64, expanded: bool, preset: SizePreset) -> Element<'a, Message> {
    // U+25B2/25BC (full-size triangles): the "small triangle" variants
    // (U+25B4/25BE) render tiny regardless of font size.
    let label = if expanded { "▲" } else { "▼" };
    button(text(label).size(preset.menu_button_text_size()))
        .padding(preset.menu_button_padding())
        .on_press(Message::ToggleMenu(note_id))
        .style(plain_button_style)
        .into()
}

fn options_row<'a>(note: &'a Note, preset: SizePreset) -> Element<'a, Message> {
    let swatches = NoteColor::ALL
        .iter()
        .fold(row![].spacing(preset.swatch_spacing()), |row, &color| {
            row.push(color_swatch(note.id, color, preset))
        });

    let pin_label = if note.always_visible { "📌" } else { "📍" };
    let pin = button(text(pin_label).size(preset.icon_text_size()))
        .padding(preset.icon_button_padding())
        .on_press(Message::ToggleAlwaysVisible(note.id))
        .style(plain_button_style);

    // Cycles the note to the next output (monitor); no-op (per app.rs) if
    // fewer than two were enumerated at startup.
    let move_output = button(text("🖥").size(preset.icon_text_size()))
        .padding(preset.icon_button_padding())
        .on_press(Message::MoveToNextOutput(note.id))
        .style(plain_button_style);

    // Rebind the note to the currently active program.
    let rebind = button(text("🔗").size(preset.icon_text_size()))
        .padding(preset.icon_button_padding())
        .on_press(Message::RebindApp(note.id))
        .style(plain_button_style);

    let trash = button(text("🗑").size(preset.icon_text_size()))
        .padding(preset.icon_button_padding())
        .on_press(Message::DeleteNote(note.id))
        .style(plain_button_style);

    row![swatches, pin, move_output, rebind, trash]
        .spacing(preset.options_row_spacing())
        .padding(preset.options_row_padding())
        .align_y(Alignment::Center)
        .into()
}

fn color_swatch<'a>(note_id: u64, color: NoteColor, preset: SizePreset) -> Element<'a, Message> {
    let size = preset.swatch_size() as f32;
    button(iced::widget::space::horizontal().width(size).height(size))
        .padding(preset.swatch_padding())
        .on_press(Message::ColorChanged(note_id, color))
        .style(move |_theme, _status| button::Style {
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
