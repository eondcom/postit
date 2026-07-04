//! Global, app-wide user settings — currently just the note size preset.
//! Persisted separately from `notes.json` (see `storage.rs`) since these are
//! process-wide preferences, not per-note data.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// The two note size presets a user can pick from the tray's "크기" submenu.
/// `Default` matches postit's original fixed sizes; `Small` shrinks note
/// dimensions and text/control sizes for users who want denser notes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum SizePreset {
    #[default]
    Default,
    Small,
}

/// Notes are square-cornered rectangles whose right-edge width can never
/// shrink below `min_note_width()` (see below) but may grow arbitrarily large
/// via the resize handle; this cap keeps that from being unbounded. Shared by
/// both presets, unlike every other metric here.
pub const MAX_NOTE_WIDTH: i32 = 800;

impl SizePreset {
    /// Collapsed (menu closed) note surface height, logical px.
    pub fn note_height(&self) -> u32 {
        match self {
            SizePreset::Default => 40,
            SizePreset::Small => 30,
        }
    }

    /// Expanded (inline menu open) note surface height, logical px.
    pub fn note_expanded_height(&self) -> u32 {
        match self {
            SizePreset::Default => 76,
            SizePreset::Small => 60,
        }
    }

    /// Width assigned to newly-created notes, logical px.
    pub fn default_note_width(&self) -> i32 {
        match self {
            SizePreset::Default => 152,
            SizePreset::Small => 120,
        }
    }

    /// The narrowest a note is ever allowed to be (resize-handle lower clamp,
    /// and the floor applied when the inline menu is expanded): just wide
    /// enough that the options row — 6 color swatches, then 📌/🖥/🔗/🗑 — never
    /// clips or wraps.
    ///
    /// Derived by summing `note_view::options_row`'s actual widget widths for
    /// this preset, rather than guessed:
    ///
    /// ```text
    /// swatch button width  = swatch_size() + 2*swatch_padding()
    /// icon button width    = icon_text_size() + 2*icon_button_padding()
    /// inner content width  = 6 * swatch_button + 5*swatch_spacing()   (the swatches row)
    ///                      + 4 * icon_button                          (pin/monitor/link/trash)
    /// row gaps             = 4 * options_row_spacing()  (5 children: swatches, pin, 🖥, 🔗, 🗑)
    /// row h-padding        = 2 * options_row_h_padding()
    /// total                = inner content width + row gaps + row h-padding
    /// ```
    ///
    /// Default: swatch btn = 14+2*1=16, icon btn = 14+2*4=22.
    ///   inner = 6*16 + 5*4 (=116) + 4*22 (=88) = 204
    ///   gaps  = 4*6 = 24; h-padding = 2*6 = 12
    ///   total = 204 + 24 + 12 = **240** (exact, no safety margin needed).
    ///
    /// Small: swatch btn = 11+2*1=13, icon btn = 11+2*3=17.
    ///   inner = 6*13 + 5*3 (=93) + 4*17 (=68) = 161
    ///   gaps  = 4*5 = 20; h-padding = 2*5 = 10
    ///   total = 161 + 20 + 10 = 191, rounded up to **200** — text/emoji glyph
    ///   advance widths can run a little wider than the nominal font size
    ///   used above, so a ~9px buffer avoids clipping in practice.
    pub fn min_note_width(&self) -> i32 {
        match self {
            SizePreset::Default => 240,
            SizePreset::Small => 200,
        }
    }

    /// Main note-text (`text_input`) font size.
    pub fn text_size(&self) -> u32 {
        match self {
            SizePreset::Default => 14,
            SizePreset::Small => 11,
        }
    }

    /// Padding inside the main note-text `text_input`.
    pub fn input_padding(&self) -> u16 {
        match self {
            SizePreset::Default => 4,
            SizePreset::Small => 3,
        }
    }

    /// Font size of the ▾/▴ menu-toggle button label.
    pub fn menu_button_text_size(&self) -> u32 {
        match self {
            SizePreset::Default => 12,
            SizePreset::Small => 10,
        }
    }

    /// Padding around the ▾/▴ menu-toggle button.
    pub fn menu_button_padding(&self) -> u16 {
        match self {
            SizePreset::Default => 4,
            SizePreset::Small => 3,
        }
    }

    /// Side length of each color swatch square (before padding/border).
    pub fn swatch_size(&self) -> u16 {
        match self {
            SizePreset::Default => 14,
            SizePreset::Small => 11,
        }
    }

    /// Padding around each color swatch button.
    pub fn swatch_padding(&self) -> u16 {
        match self {
            SizePreset::Default => 1,
            SizePreset::Small => 1,
        }
    }

    /// Spacing between adjacent color swatches.
    pub fn swatch_spacing(&self) -> u32 {
        match self {
            SizePreset::Default => 4,
            SizePreset::Small => 3,
        }
    }

    /// Font size used for the 📌/🖥/🔗/🗑 icon buttons in the options row.
    pub fn icon_text_size(&self) -> u32 {
        match self {
            SizePreset::Default => 14,
            SizePreset::Small => 11,
        }
    }

    /// Padding around each of the 📌/🖥/🔗/🗑 icon buttons.
    pub fn icon_button_padding(&self) -> u16 {
        match self {
            SizePreset::Default => 4,
            SizePreset::Small => 3,
        }
    }

    /// Spacing between the swatches group and each icon button in the
    /// options row (also used as the row's `.spacing()`).
    pub fn options_row_spacing(&self) -> u32 {
        match self {
            SizePreset::Default => 6,
            SizePreset::Small => 5,
        }
    }

    /// The options row's `.padding([vertical, horizontal])`.
    pub fn options_row_padding(&self) -> [u16; 2] {
        match self {
            SizePreset::Default => [2, 6],
            SizePreset::Small => [2, 5],
        }
    }

    /// Width of the left-edge drag grip.
    pub fn grip_width(&self) -> f32 {
        match self {
            SizePreset::Default => 12.0,
            SizePreset::Small => 9.0,
        }
    }

    /// Font size of the "⣿" glyph drawn inside the drag grip.
    pub fn grip_icon_text_size(&self) -> u32 {
        match self {
            SizePreset::Default => 12,
            SizePreset::Small => 9,
        }
    }

    /// Width of the right-edge resize handle.
    pub fn resize_handle_width(&self) -> f32 {
        match self {
            SizePreset::Default => 10.0,
            SizePreset::Small => 8.0,
        }
    }
}

/// Default note opacity, percent (0..=100). Applied to both the note's
/// background and border colors; see `AppSettings::opacity_alpha`.
fn default_opacity() -> u8 {
    100
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct AppSettings {
    #[serde(default)]
    pub size_preset: SizePreset,
    /// Global note opacity, percent (0..=100), set from the tray's "투명도"
    /// submenu. Applied uniformly to every note surface.
    #[serde(default = "default_opacity")]
    pub opacity: u8,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            size_preset: SizePreset::default(),
            opacity: default_opacity(),
        }
    }
}

impl AppSettings {
    /// `opacity` (0..=100) as a 0.0..=1.0 alpha multiplier, for background /
    /// border colors. Note text keeps its own higher floor for readability
    /// (see `note_view::text_alpha`), rather than using this directly.
    pub fn opacity_alpha(&self) -> f32 {
        self.opacity.min(100) as f32 / 100.0
    }
}

fn get_settings_path() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("postit").join("settings.json")
    } else {
        PathBuf::from(".local/share/postit/settings.json")
    }
}

/// Loads settings from disk; any failure (missing file, unreadable, bad
/// JSON) silently falls back to `AppSettings::default()`, same policy as
/// `storage::load_notes`.
pub fn load_settings() -> AppSettings {
    let path = get_settings_path();
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("Failed to parse settings.json: {}", e);
            AppSettings::default()
        }),
        Err(_) => AppSettings::default(),
    }
}

/// Saves settings via the same atomic write-tmp-then-rename pattern as
/// `storage::save_notes`, so a crash mid-write can never corrupt the file.
pub fn save_settings(settings: &AppSettings) {
    let path = get_settings_path();

    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("Failed to create directory for settings: {}", e);
            return;
        }
    }

    let tmp_path = path.with_file_name("settings.json.tmp");

    let json_string = match serde_json::to_string_pretty(settings) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize settings: {}", e);
            return;
        }
    };

    if let Err(e) = fs::write(&tmp_path, json_string) {
        eprintln!("Failed to write temporary settings file: {}", e);
        return;
    }

    if let Err(e) = fs::rename(&tmp_path, &path) {
        eprintln!("Failed to rename settings file: {}", e);
        let _ = fs::remove_file(&tmp_path);
    }
}
