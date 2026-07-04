//! System tray icon (StatusNotifierItem, via `ksni`) — the primary way to
//! spawn notes and reach the note list / quit action, replacing the floating
//! toolbar on compositors that expose a status-area applet (e.g. COSMIC).
//!
//! Mirrors the thread+channel pattern used by `focus.rs`: `spawn()` starts a
//! dedicated OS thread and returns the receiving end of a `std::sync::mpsc`
//! channel. `ksni::blocking::TrayMethods::spawn` itself blocks synchronously
//! until the D-Bus connection + `StatusNotifierWatcher` registration either
//! succeeds or fails, so by the time that call returns we already know
//! whether the tray icon exists; we forward that as a one-shot
//! `TrayMessage::Registered`/`TrayMessage::Unavailable` and, from then on,
//! forward menu/activation events as `TrayMessage::Event`.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem, SubMenu};

use crate::colors::NoteColor;
use crate::settings::SizePreset;

/// Events a user can trigger from the tray icon or its context menu.
#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    NewNote(NoteColor),
    RefreshOutputs,
    ShowList,
    /// Picked from the "크기" submenu: switch every note to this size preset.
    SetSizePreset(SizePreset),
    /// Picked from the "투명도" submenu: set every note's opacity, percent
    /// (0..=100).
    SetOpacity(u8),
    Quit,
}

/// Everything the tray thread reports back to the app.
#[derive(Debug, Clone, Copy)]
pub enum TrayMessage {
    /// The tray icon was successfully registered with the StatusNotifierWatcher.
    Registered,
    /// Registration failed (no watcher, no D-Bus, etc.); the caller should
    /// fall back to the floating toolbar.
    Unavailable,
    Event(TrayEvent),
}

struct PostitTray {
    tx: Sender<TrayMessage>,
}

impl ksni::Tray for PostitTray {
    fn id(&self) -> String {
        "postit".into()
    }

    fn title(&self) -> String {
        "postit".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![note_icon()]
    }

    /// Left click (the "primary activation") on the tray icon: spawn a
    /// yellow note directly, without going through the menu.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self
            .tx
            .send(TrayMessage::Event(TrayEvent::NewNote(NoteColor::Yellow)));
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let new_note_items: Vec<MenuItem<Self>> = NoteColor::ALL
            .iter()
            .copied()
            .map(|color| {
                StandardItem {
                    label: color_label(color).to_string(),
                    activate: Box::new(move |this: &mut Self| {
                        let _ = this
                            .tx
                            .send(TrayMessage::Event(TrayEvent::NewNote(color)));
                    }),
                    ..Default::default()
                }
                .into()
            })
            .collect();

        vec![
            SubMenu {
                label: "새 포스트잇".into(),
                submenu: new_note_items,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "모니터 새로읽기".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this
                        .tx
                        .send(TrayMessage::Event(TrayEvent::RefreshOutputs));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "포스트잇 목록".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayMessage::Event(TrayEvent::ShowList));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "크기".into(),
                submenu: vec![
                    StandardItem {
                        label: "기본".into(),
                        activate: Box::new(|this: &mut Self| {
                            let _ = this.tx.send(TrayMessage::Event(TrayEvent::SetSizePreset(
                                SizePreset::Default,
                            )));
                        }),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "스몰".into(),
                        activate: Box::new(|this: &mut Self| {
                            let _ = this.tx.send(TrayMessage::Event(TrayEvent::SetSizePreset(
                                SizePreset::Small,
                            )));
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "투명도".into(),
                submenu: [100u8, 90, 80, 70, 60]
                    .into_iter()
                    .map(|pct| {
                        StandardItem {
                            label: format!("{pct}%"),
                            activate: Box::new(move |this: &mut Self| {
                                let _ = this
                                    .tx
                                    .send(TrayMessage::Event(TrayEvent::SetOpacity(pct)));
                            }),
                            ..Default::default()
                        }
                        .into()
                    })
                    .collect(),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "종료".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayMessage::Event(TrayEvent::Quit));
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn color_label(color: NoteColor) -> &'static str {
    match color {
        NoteColor::Yellow => "노랑",
        NoteColor::Pink => "핑크",
        NoteColor::Blue => "파랑",
        NoteColor::Green => "그린",
        NoteColor::Orange => "오렌지",
        NoteColor::Gray => "회색",
    }
}

/// Builds a 22x22 ARGB32 (network byte order: A,R,G,B per pixel) pixmap of a
/// simple yellow sticky note with a folded bottom-right corner, so the tray
/// icon never depends on the system icon theme having a matching name.
fn note_icon() -> ksni::Icon {
    const SIZE: i32 = 22;
    const FOLD: i32 = 7;
    const BG: [u8; 3] = [255, 241, 118]; // postit yellow
    const BORDER: [u8; 3] = [196, 169, 10];
    const FOLD_FACE: [u8; 3] = [222, 196, 64];
    const FOLD_EDGE: [u8; 3] = [170, 145, 20];

    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let is_border = x == 0 || y == 0 || x == SIZE - 1 || y == SIZE - 1;

            // The folded corner lives in the bottom-right FOLD x FOLD square,
            // split along its anti-diagonal into the "flap" (far corner) and
            // the rest of the note.
            let in_fold_square = x >= SIZE - FOLD && y >= SIZE - FOLD;
            let lx = x - (SIZE - FOLD);
            let ly = y - (SIZE - FOLD);
            let on_crease = in_fold_square && lx + ly == FOLD - 1;
            let in_flap = in_fold_square && lx + ly > FOLD - 1;

            let rgb = if on_crease {
                FOLD_EDGE
            } else if in_flap {
                FOLD_FACE
            } else if is_border {
                BORDER
            } else {
                BG
            };

            let idx = ((y * SIZE + x) * 4) as usize;
            data[idx] = 255; // A
            data[idx + 1] = rgb[0]; // R
            data[idx + 2] = rgb[1]; // G
            data[idx + 3] = rgb[2]; // B
        }
    }

    ksni::Icon {
        width: SIZE,
        height: SIZE,
        data,
    }
}

/// Spawns the tray-icon thread and returns the receiving end of the channel
/// it reports on. Registration is attempted once; on failure a warning is
/// printed and `TrayMessage::Unavailable` is sent so the caller can fall back
/// to the floating toolbar. On success, `TrayMessage::Registered` is sent and
/// the tray keeps running for the lifetime of the process (the actual D-Bus
/// event loop runs on ksni's own background thread; this thread's only job
/// is the one-time registration and forwarding menu/activation events).
pub fn spawn() -> Receiver<TrayMessage> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let tray = PostitTray { tx: tx.clone() };
        match tray.spawn() {
            Ok(handle) => {
                let _ = tx.send(TrayMessage::Registered);
                // Nothing left for this thread to do: `handle` is just a
                // (Weak-backed) remote control we don't need, the real event
                // loop lives on ksni's own thread. Drop it and exit.
                drop(handle);
            }
            Err(err) => {
                eprintln!(
                    "[postit] tray icon unavailable ({err}); falling back to floating toolbar"
                );
                let _ = tx.send(TrayMessage::Unavailable);
            }
        }
    });
    rx
}
