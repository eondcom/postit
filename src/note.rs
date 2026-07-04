use serde::{Deserialize, Serialize};
use crate::colors::NoteColor;

/// Default note width in logical px, per plan 9.2 — equal to the collapsed
/// note width (`NOTE_COLLAPSED.0` in `app.rs`) so legacy notes with no
/// recorded width render exactly as before.
pub fn default_width() -> i32 {
    152
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: u64,
    pub text: String,
    pub color: NoteColor,
    pub x: i32,
    pub y: i32,
    pub always_visible: bool,
    pub bound_app: Option<String>,
    /// 노트가 붙어 있는 출력(모니터)의 이름 (예: "DP-1"). None = 미지정(레거시 데이터).
    #[serde(default)]
    pub output: Option<String>,
    /// 노트 서피스 폭 (logical px). 우측 리사이즈 핸들로 조절, 100..=800 범위.
    #[serde(default = "default_width")]
    pub width: i32,
}

impl Note {
    pub fn new(id: u64, color: NoteColor, x: i32, y: i32, bound_app: Option<String>) -> Self {
        Note {
            id,
            text: String::new(),
            color,
            x,
            y,
            always_visible: false,
            bound_app,
            output: None,
            width: default_width(),
        }
    }
}
