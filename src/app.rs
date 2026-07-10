use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iced::window;
use iced::{event, keyboard, mouse, Element, Event, Point, Subscription, Task};

use iced_layershell::daemon;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::to_layer_message;

use crate::colors::NoteColor;
use crate::note::Note;
use crate::outputs::OutputInfo;
use crate::settings::{AppSettings, SizePreset, MAX_NOTE_WIDTH};
use crate::tray::{TrayEvent, TrayMessage};
use crate::{focus, list_view, note_view, outputs, settings, storage, toolbar, tray};

/// Fallback toolbar, only ever created if the tray icon fails to register.
/// Wide enough for 5 color swatches plus the ☰ (list) and ✕ (quit) buttons
/// that a working tray would otherwise expose via its context menu.
const TOOLBAR_SIZE: (u32, u32) = (300, 36);
/// The "포스트잇 목록" panel, toggled from the tray menu or the fallback
/// toolbar's ☰ button.
const LIST_SIZE: (u32, u32) = (320, 300);
/// postit's own app_id / namespace: while it's the active window we must not
/// update the visibility judgement (editing a note shouldn't hide it).
const SELF_APP_ID: &str = "postit";

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub enum Message {
    CreateNote(NoteColor),
    TextChanged(u64, String),
    ToggleMenu(u64),
    ColorChanged(u64, NoteColor),
    ToggleAlwaysVisible(u64),
    DeleteNote(u64),
    /// Cycles a note to the next output (monitor) in left-to-right order,
    /// wrapping back to the first; position resets to (160, 160). No-op if
    /// fewer than two outputs were enumerated at startup.
    MoveToNextOutput(u64),
    /// Rebind a note's `bound_app` to the currently active program. Closes the
    /// menu if open and persists the change.
    RebindApp(u64),
    DragStart(u64),
    /// Pressing the right-edge resize handle (`note_view::resize_handle`)
    /// starts a width resize, mutually exclusive with `DragStart`.
    ResizeStart(u64),
    WindowClosed(window::Id),
    IcedEvent(window::Id, Event),
    ActiveAppChanged(String),
    /// Messages forwarded from the tray-icon thread (`tray.rs`): initial
    /// registration result, then activation/menu events.
    TrayMessage(TrayMessage),
    /// Toggles the "포스트잇 목록" panel open/closed.
    ToggleList,
    /// Cycles the "포스트잇 목록" panel to the next output (monitor) in
    /// left-to-right order, wrapping back to the first. No-op if fewer than
    /// two outputs were enumerated at startup, or if the list panel is
    /// currently closed.
    MoveListToNextOutput,
    /// Pressing the drag grip on the "포스트잇 목록" panel's header starts a
    /// free-form drag of the whole panel, mirroring `DragStart` for notes
    /// (see `DragTarget::List`).
    ListDragStart,
    /// Brings a note back to a known on-screen position, creating its
    /// surface first if the note is currently hidden.
    ImportNote(u64),
    /// Sets the global note size preset (tray "크기" submenu), persists it,
    /// and resizes every currently-open note surface to match.
    SetSizePreset(SizePreset),
    /// Sets the global note opacity percent (tray "투명도" submenu) and
    /// persists it. No surface resize needed — opacity is read straight from
    /// `settings` by `note_view::view` on the next render.
    SetOpacity(u8),
    /// Saves and terminates the whole application.
    Quit,
}

/// What a `DragState` is moving: either a specific note, or the "포스트잇
/// 목록" panel (which has no note id of its own).
#[derive(Clone, Copy)]
enum DragTarget {
    Note(u64),
    List,
}

struct DragState {
    target: DragTarget,
    surface_id: window::Id,
    /// Position (x, y) at the moment the grip was grabbed — the note's
    /// `(x, y)` for `DragTarget::Note`, or `list_pos` for `DragTarget::List`.
    start: (i32, i32),
    /// Cursor point of the first motion event after the grab. During the
    /// implicit pointer grab the compositor keeps reporting coordinates
    /// relative to the grab-time surface position, so
    /// `start + (position - press)` is the absolute target position.
    /// Deriving it absolutely (instead of accumulating per-event deltas)
    /// keeps any coordinate-model mismatch bounded — it can never compound
    /// into runaway movement. This applies identically whether the target is
    /// a note or the list panel.
    /// `None` also right after a mid-drag output hop (see `cross_output_hop`
    /// and `note_drag_motion`/`list_drag_motion`): the surface is destroyed
    /// and recreated under the cursor, which breaks the implicit grab, but
    /// as long as the new surface is created positioned under the pointer
    /// the compositor re-enters it and keeps delivering motion events with
    /// the button still down. Re-anchoring `start`/`surface_id`/`press` here
    /// (`press` reset to `None` so the next motion event re-establishes it,
    /// same as a fresh `DragStart`) is what makes the drag feel continuous
    /// across the monitor boundary instead of requiring a release-and-hop.
    press: Option<Point>,
    /// Rate-limits margin commits to roughly once per frame.
    last_apply: Option<Instant>,
}

/// Mirrors `DragState`'s absolute-coordinate approach (see its doc comment)
/// but for the right-edge resize handle: width, not position, changes.
struct ResizeState {
    note_id: u64,
    surface_id: window::Id,
    /// Note width at the moment the resize handle was grabbed.
    start_width: i32,
    /// Cursor point of the first motion event after the grab. New width is
    /// always derived as `start_width + (position.x - press.x)` — never
    /// accumulated per-event — for the same reason `DragState::press` is
    /// documented: it keeps any coordinate-model mismatch bounded instead of
    /// letting it compound into runaway growth/shrinkage.
    press: Option<Point>,
    /// Rate-limits size commits to roughly once per frame.
    last_apply: Option<Instant>,
}

pub struct PostitApp {
    notes: HashMap<u64, Note>,
    /// surface id -> note id, for every currently-visible note surface.
    surfaces: HashMap<window::Id, u64>,
    /// note id -> surface id, the inverse of `surfaces`.
    note_surface: HashMap<u64, window::Id>,
    /// note ids whose inline menu is currently expanded.
    menu_open: HashSet<u64>,
    drag: Option<DragState>,
    /// Active right-edge width resize, if any. Mutually exclusive with
    /// `drag`: starting one clears the other (see `Message::DragStart` and
    /// `Message::ResizeStart` handlers).
    resize: Option<ResizeState>,
    /// Last known non-postit active app_id, per the app-binding rules.
    active_app: Option<String>,
    /// Surface id of the "포스트잇 목록" panel, if currently open.
    list_surface: Option<window::Id>,
    /// Output name where the "포스트잇 목록" panel is displayed. `None`
    /// means the compositor chooses.
    list_output: Option<String>,
    /// Current position (x, y) of the "포스트잇 목록" panel, as a margin
    /// against `Anchor::Top | Anchor::Left`. Not persisted — resets to
    /// `LIST_DEFAULT_POS` on every app restart and whenever the panel hops to
    /// another output via `Message::MoveListToNextOutput`.
    list_pos: (i32, i32),
    /// Surface id of the fallback floating toolbar, if it had to be created
    /// because the tray icon could not be registered. `None` both before
    /// that's known and while the tray is working fine.
    toolbar_surface: Option<window::Id>,
    /// Outputs (monitors) enumerated at startup via
    /// `outputs::list_outputs()`, sorted left-to-right. Re-scanned on demand
    /// through the tray's "모니터 새로읽기" menu item (`refresh_outputs`) —
    /// there is no automatic hot-plug tracking; see plan 8.3.
    outputs: Vec<OutputInfo>,
    /// Global, persisted user preferences (size preset, opacity). Loaded once
    /// at startup via `settings::load_settings`, saved on every change.
    settings: AppSettings,
}

fn new_note_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Mid-drag monitor-boundary check, shared by `PostitApp::note_drag_motion`
/// and `PostitApp::list_drag_motion`. `target_x`/`target_y` are the raw
/// (unclamped) proposed position in the current output's coordinate space;
/// `width`/`height` are the dragged thing's size (a note's `width` and the
/// active preset's `note_height()`, or `LIST_SIZE`). `current_output` is the
/// name of the output the drag is currently on — `None` is treated as index
/// 0, matching the legacy/not-yet-placed convention used elsewhere (see
/// `maybe_hop_list_output`).
///
/// Returns `Some((new_output_index, new_x, new_y))` once the dragged item's
/// centerline has crossed past the current output's left or right edge and
/// an adjacent output exists in that direction — the new position is
/// expressed in the *new* output's coordinate space, y carried over and
/// clamped to its height. Returns `None` when the drag should just clamp in
/// place instead (no crossing yet, or no output to cross onto).
fn cross_output_hop(
    outputs: &[OutputInfo],
    current_output: Option<&str>,
    target_x: i32,
    target_y: i32,
    width: i32,
    height: i32,
) -> Option<(usize, i32, i32)> {
    if outputs.len() < 2 {
        return None;
    }
    let current_index = current_output
        .and_then(|name| outputs.iter().position(|o| o.name == name))
        .unwrap_or(0);
    let current = &outputs[current_index];

    if target_x + width / 2 > current.width {
        if current_index + 1 >= outputs.len() {
            return None;
        }
        let next = &outputs[current_index + 1];
        let new_x = (target_x - current.width).max(0);
        let new_y = target_y.clamp(0, (next.height - height).max(0));
        return Some((current_index + 1, new_x, new_y));
    }

    if target_x + width / 2 < 0 {
        if current_index == 0 {
            return None;
        }
        let prev = &outputs[current_index - 1];
        let new_x = (prev.width + target_x).min(prev.width - width).max(0);
        let new_y = target_y.clamp(0, (prev.height - height).max(0));
        return Some((current_index - 1, new_x, new_y));
    }

    None
}

/// Resolves a note's recorded output name to a concrete `OutputOption`.
/// Falls back to `OutputOption::None` (compositor picks) if the note has no
/// recorded output, or that output is no longer among the ones enumerated at
/// startup (e.g. it was unplugged) — same as the legacy/enumeration-failed
/// case.
fn resolve_output_option(note: &Note, outputs: &[OutputInfo]) -> OutputOption {
    match &note.output {
        Some(name) if outputs.iter().any(|o| &o.name == name) => {
            OutputOption::OutputName(name.clone())
        }
        _ => OutputOption::None,
    }
}

fn note_layer_settings(
    note: &Note,
    expanded: bool,
    outputs: &[OutputInfo],
    preset: SizePreset,
) -> NewLayerShellSettings {
    // Height is fixed per the active size preset; width is per-note and
    // user-adjustable (plan 9.2).
    let height = if expanded {
        preset.note_expanded_height()
    } else {
        preset.note_height()
    };
    let size = (note.width as u32, height);
    NewLayerShellSettings {
        size: Some(size),
        layer: Layer::Top,
        anchor: Anchor::Top | Anchor::Left,
        exclusive_zone: Some(0),
        // layershellev margin order is CSS-like: (top, right, bottom, left)
        margin: Some((note.y, 0, 0, note.x)),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        output_option: resolve_output_option(note, outputs),
        events_transparent: false,
        namespace: Some("postit-note".to_string()),
    }
}

/// Layer settings for the fallback floating toolbar. Only ever instantiated
/// on demand, when the tray thread reports it could not register (see
/// `handle_tray_message`) — the daemon itself starts with `StartMode::Background`,
/// so no toolbar surface exists unless this is used.
fn toolbar_layer_settings() -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some(TOOLBAR_SIZE),
        layer: Layer::Top,
        anchor: Anchor::Top,
        exclusive_zone: Some(0),
        margin: Some((8, 0, 0, 0)),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: OutputOption::None,
        events_transparent: false,
        namespace: Some("postit-toolbar".to_string()),
    }
}

/// Default position (x, y) for the "포스트잇 목록" panel: on first open, and
/// whenever it's moved to another output via the 🖥 button.
const LIST_DEFAULT_POS: (i32, i32) = (200, 48);

/// Layer settings for the "포스트잇 목록" panel. Anchored `Top | Left` with a
/// margin derived from `pos`, same pattern as `note_layer_settings` — this is
/// what makes the panel freely draggable instead of compositor-centered.
fn list_layer_settings(
    list_output: &Option<String>,
    outputs: &[OutputInfo],
    pos: (i32, i32),
) -> NewLayerShellSettings {
    let output_option = match list_output {
        Some(name) if outputs.iter().any(|o| &o.name == name) => {
            OutputOption::OutputName(name.clone())
        }
        _ => OutputOption::None,
    };
    NewLayerShellSettings {
        size: Some(LIST_SIZE),
        layer: Layer::Top,
        anchor: Anchor::Top | Anchor::Left,
        exclusive_zone: Some(0),
        // layershellev margin order is CSS-like: (top, right, bottom, left)
        margin: Some((pos.1, 0, 0, pos.0)),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option,
        events_transparent: false,
        namespace: Some("postit-list".to_string()),
    }
}

impl PostitApp {
    fn new() -> (Self, Task<Message>) {
        let app_settings = settings::load_settings();
        let mut loaded = storage::load_notes();
        // One-shot enumeration over its own short-lived Wayland connection;
        // see `outputs.rs`. Empty on failure, which every output-aware call
        // site below treats as "fall back to compositor-chosen placement".
        let outputs = outputs::list_outputs();

        // Migrate legacy notes (saved before multi-monitor support existed)
        // onto the leftmost output, per plan 8.3 point 5.
        let mut migrated = false;
        if !outputs.is_empty() {
            for note in &mut loaded {
                if note.output.is_none() {
                    note.output = Some(outputs[0].name.clone());
                    migrated = true;
                }
            }
        }
        if migrated {
            storage::save_notes(&loaded);
        }

        let mut notes = HashMap::new();
        let mut surfaces = HashMap::new();
        let mut note_surface = HashMap::new();
        let mut tasks = Vec::new();

        for note in loaded {
            let surface_id = window::Id::unique();
            surfaces.insert(surface_id, note.id);
            note_surface.insert(note.id, surface_id);
            tasks.push(Task::done(Message::NewLayerShell {
                settings: note_layer_settings(&note, false, &outputs, app_settings.size_preset),
                id: surface_id,
            }));
            notes.insert(note.id, note);
        }

        (
            PostitApp {
                notes,
                surfaces,
                note_surface,
                menu_open: HashSet::new(),
                drag: None,
                resize: None,
                active_app: None,
                list_surface: None,
                list_output: None,
                list_pos: LIST_DEFAULT_POS,
                toolbar_surface: None,
                outputs,
                settings: app_settings,
            },
            Task::batch(tasks),
        )
    }

    fn namespace() -> String {
        "postit".to_string()
    }

    fn save(&self) {
        let list: Vec<Note> = self.notes.values().cloned().collect();
        storage::save_notes(&list);
    }

    fn should_be_visible(note: &Note, active_app: &Option<String>) -> bool {
        if note.always_visible {
            return true;
        }
        match (&note.bound_app, active_app) {
            (Some(bound), Some(active)) => bound == active,
            // Unknown binding or unknown active app: default to visible.
            _ => true,
        }
    }

    /// Creates/removes note surfaces so that exactly the notes that should be
    /// visible (per `should_be_visible`) currently have a surface.
    fn reconcile_visibility(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let note_ids: Vec<u64> = self.notes.keys().copied().collect();

        for note_id in note_ids {
            let should_show = Self::should_be_visible(&self.notes[&note_id], &self.active_app);
            let currently_shown = self.note_surface.contains_key(&note_id);

            if should_show && !currently_shown {
                let surface_id = window::Id::unique();
                self.surfaces.insert(surface_id, note_id);
                self.note_surface.insert(note_id, surface_id);
                let expanded = self.menu_open.contains(&note_id);
                let settings = note_layer_settings(
                    &self.notes[&note_id],
                    expanded,
                    &self.outputs,
                    self.settings.size_preset,
                );
                tasks.push(Task::done(Message::NewLayerShell {
                    settings,
                    id: surface_id,
                }));
            } else if !should_show && currently_shown {
                if let Some(surface_id) = self.note_surface.remove(&note_id) {
                    self.surfaces.remove(&surface_id);
                    tasks.push(Task::done(Message::RemoveWindow(surface_id)));
                }
            }
        }

        Task::batch(tasks)
    }

    fn handle_iced_event(&mut self, id: window::Id, event: Event) -> Task<Message> {
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if self.drag.is_some() {
                    // Scoped immutable borrow: extract everything needed as
                    // owned values so it ends before we call the `&mut self`
                    // motion handlers below (which may recreate a surface
                    // and replace `self.drag` entirely on a mid-drag hop).
                    let drag = self.drag.as_ref().expect("checked Some above");
                    if drag.surface_id != id {
                        return Task::none();
                    }
                    let Some(press) = drag.press else {
                        self.drag.as_mut().expect("checked Some above").press = Some(position);
                        return Task::none();
                    };
                    let due = drag
                        .last_apply
                        .is_none_or(|t| t.elapsed() >= Duration::from_millis(8));
                    if !due {
                        return Task::none();
                    }
                    let dx = (position.x - press.x).round() as i32;
                    let dy = (position.y - press.y).round() as i32;
                    // Raw (unclamped) target, computed before any `.max(0)`
                    // clamping so `cross_output_hop` can see genuine
                    // out-of-bounds overshoot past either edge.
                    let target_x = drag.start.0 + dx;
                    let target_y = drag.start.1 + dy;
                    let target = drag.target;

                    return match target {
                        DragTarget::Note(note_id) => self.note_drag_motion(id, note_id, target_x, target_y),
                        DragTarget::List => self.list_drag_motion(id, target_x, target_y),
                    };
                }
                if let Some(resize) = &mut self.resize {
                    if resize.surface_id == id {
                        let Some(press) = resize.press else {
                            resize.press = Some(position);
                            return Task::none();
                        };
                        let due = resize
                            .last_apply
                            .is_none_or(|t| t.elapsed() >= Duration::from_millis(8));
                        if !due {
                            return Task::none();
                        }
                        let dx = (position.x - press.x).round() as i32;
                        let target_width = (resize.start_width + dx)
                            .clamp(self.settings.size_preset.min_note_width(), MAX_NOTE_WIDTH);
                        let note_id = resize.note_id;
                        let expanded = self.menu_open.contains(&note_id);
                        let preset = self.settings.size_preset;
                        if let Some(note) = self.notes.get_mut(&note_id) {
                            if note.width != target_width {
                                resize.last_apply = Some(Instant::now());
                                note.width = target_width;
                                let height = if expanded {
                                    preset.note_expanded_height()
                                } else {
                                    preset.note_height()
                                };
                                return Task::done(Message::SizeChange {
                                    id,
                                    size: (target_width as u32, height),
                                });
                            }
                        }
                    }
                }
                Task::none()
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(drag) = self.drag.take() {
                    // Continuous mid-drag hops (`note_drag_motion` /
                    // `list_drag_motion`) handle crossing a monitor boundary
                    // while the button is still down. This release-time hop
                    // is a fallback for the case where the drag ends exactly
                    // flush against an edge without ever having overshot it.
                    let task = match drag.target {
                        DragTarget::Note(note_id) => {
                            let task = self.maybe_hop_output(note_id);
                            self.save();
                            task
                        }
                        // The list panel's position isn't persisted, so
                        // there's nothing to save — just the same edge-hop
                        // judgement as notes get.
                        DragTarget::List => self.maybe_hop_list_output(),
                    };
                    return task;
                }
                if self.resize.take().is_some() {
                    // Width was already committed onto `note.width` on every
                    // applied `CursorMoved` above; this just persists it.
                    self.save();
                }
                Task::none()
            }
            // Only reaches here when nothing captured the key (see the
            // `event::Status::Ignored` guard in `subscription`) — i.e. the
            // note's grip/resize-handle/menu was clicked rather than its
            // text field, so there's no focused `text_input` to already
            // handle Del itself.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Delete),
                ..
            }) => {
                let Some(&note_id) = self.surfaces.get(&id) else {
                    return Task::none();
                };
                if self.notes.get(&note_id).is_some_and(|note| note.text.is_empty()) {
                    return self.update(Message::DeleteNote(note_id));
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Handles one `CursorMoved` sample of a note drag: either continues the
    /// hop onto an adjacent output (see `cross_output_hop`) or, absent a
    /// crossing, applies the ordinary clamp-and-commit-margin behavior that
    /// existed before continuous hopping.
    ///
    /// `target_x`/`target_y` are the raw, unclamped proposed position (in
    /// the current output's coordinate space) derived from `DragState`.
    fn note_drag_motion(&mut self, surface_id: window::Id, note_id: u64, target_x: i32, target_y: i32) -> Task<Message> {
        let Some(note) = self.notes.get(&note_id) else {
            return Task::none();
        };
        let current_output = note.output.clone();
        let width = note.width;
        let height = self.settings.size_preset.note_height() as i32;

        if let Some((new_index, new_x, new_y)) = cross_output_hop(
            &self.outputs,
            current_output.as_deref(),
            target_x,
            target_y,
            width,
            height,
        ) {
            let new_output_name = self.outputs[new_index].name.clone();
            if let Some(note) = self.notes.get_mut(&note_id) {
                note.output = Some(new_output_name);
                note.x = new_x;
                note.y = new_y;
            }
            // The note's output/x/y just changed for real (unlike a
            // same-output drag sample, which only commits on release) — this
            // is the only chance to persist it before a crash or restart.
            self.save();
            let recreate_task = self.recreate_note_surface(note_id);
            // Re-anchor the drag onto the freshly created surface so the
            // still-held button keeps driving it (see `DragState::press`
            // doc comment).
            if let Some(new_surface_id) = self.note_surface.get(&note_id).copied() {
                self.drag = Some(DragState {
                    target: DragTarget::Note(note_id),
                    surface_id: new_surface_id,
                    start: (new_x, new_y),
                    press: None,
                    last_apply: None,
                });
            }
            return recreate_task;
        }

        // No crossing: same clamp-and-commit-margin behavior as before
        // continuous hopping existed.
        let clamped_x = target_x.max(0);
        let clamped_y = target_y.max(0);
        let Some(note) = self.notes.get_mut(&note_id) else {
            return Task::none();
        };
        if (note.x, note.y) != (clamped_x, clamped_y) {
            note.x = clamped_x;
            note.y = clamped_y;
            let margin = (note.y, 0, 0, note.x);
            if let Some(drag) = &mut self.drag {
                drag.last_apply = Some(Instant::now());
            }
            return Task::done(Message::MarginChange { id: surface_id, margin });
        }
        Task::none()
    }

    /// Mirrors `note_drag_motion` for the "포스트잇 목록" panel. Position
    /// isn't persisted (same as `maybe_hop_list_output`), so no `save()`.
    fn list_drag_motion(&mut self, surface_id: window::Id, target_x: i32, target_y: i32) -> Task<Message> {
        let current_output = self.list_output.clone();
        let width = LIST_SIZE.0 as i32;
        let height = LIST_SIZE.1 as i32;

        if let Some((new_index, new_x, new_y)) = cross_output_hop(
            &self.outputs,
            current_output.as_deref(),
            target_x,
            target_y,
            width,
            height,
        ) {
            let new_output_name = self.outputs[new_index].name.clone();
            self.list_output = Some(new_output_name);
            self.list_pos = (new_x, new_y);
            let recreate_task = self.recreate_list_surface();
            if let Some(new_surface_id) = self.list_surface {
                self.drag = Some(DragState {
                    target: DragTarget::List,
                    surface_id: new_surface_id,
                    start: (new_x, new_y),
                    press: None,
                    last_apply: None,
                });
            }
            return recreate_task;
        }

        let clamped_x = target_x.max(0);
        let clamped_y = target_y.max(0);
        if self.list_pos != (clamped_x, clamped_y) {
            self.list_pos = (clamped_x, clamped_y);
            let margin = (clamped_y, 0, 0, clamped_x);
            if let Some(drag) = &mut self.drag {
                drag.last_apply = Some(Instant::now());
            }
            return Task::done(Message::MarginChange { id: surface_id, margin });
        }
        Task::none()
    }

    /// If the note is now flush against the left or right edge of the output
    /// it's currently on, and an adjacent output exists in that direction,
    /// moves the note there: updates `note.output`/`x`/`y` and recreates its
    /// surface (layer surfaces can't be reassigned to a different `wl_output`
    /// or have their `OutputOption` changed in place). Per plan 8.3 point 4:
    /// right hop lands at x = 8, left hop at x = width − note_width − 8, y is
    /// carried over clamped to the new output's height. No-op if the note
    /// isn't near an edge, has no recorded/known output, or there's nothing
    /// to hop to in that direction.
    /// Re-enumerates the outputs at the time of hopping. Monitors get
    /// plugged/unplugged while the app runs, so the list cached at startup
    /// goes stale (a dock/undock would otherwise permanently disable hopping
    /// until restart). One-shot Wayland roundtrips, called from the tray menu's
    /// "모니터 새로읽기" (Refresh Outputs) action.
    fn refresh_outputs(&mut self) {
        let fresh = outputs::list_outputs();
        if !fresh.is_empty() {
            self.outputs = fresh;
        }
    }

    /// Gathers all notes to the primary (first) monitor, clamping their
    /// positions to stay within bounds, and recreates all affected note
    /// surfaces and the list panel (if open).
    fn gather_all_notes(&mut self) -> Task<Message> {
        // Re-scan outputs even if this is called without a prior RefreshOutputs
        self.refresh_outputs();

        if self.outputs.is_empty() {
            return Task::none();
        }

        let target = self.outputs[0].clone();
        let target_width = target.width;
        let target_height = target.height;

        // Pre-copy preset values to avoid borrow checker issues
        let preset = self.settings.size_preset;
        let note_height = preset.note_height() as i32;

        // Update all notes to point to the first monitor and clamp their positions
        for note in self.notes.values_mut() {
            note.output = Some(target.name.clone());
            note.x = note.x.clamp(0, (target_width - note.width).max(0));
            note.y = note.y.clamp(0, (target_height - note_height).max(0));
        }

        self.save();

        // Collect note IDs that have surfaces so we can recreate them
        let note_ids_with_surfaces: Vec<u64> = self
            .note_surface
            .keys()
            .copied()
            .collect();

        let mut tasks = Vec::new();

        // Recreate all note surfaces that exist
        for note_id in note_ids_with_surfaces {
            tasks.push(self.recreate_note_surface(note_id));
        }

        // If the list panel is open, recreate it to ensure it's on the primary monitor
        if self.list_surface.is_some() {
            self.list_output = Some(target.name.clone());
            self.list_pos = LIST_DEFAULT_POS;
            tasks.push(self.recreate_list_surface());
        }

        Task::batch(tasks)
    }

    fn maybe_hop_output(&mut self, note_id: u64) -> Task<Message> {
        if self.outputs.len() < 2 {
            return Task::none();
        }

        let Some(note) = self.notes.get(&note_id) else {
            return Task::none();
        };
        let Some(current_index) = note
            .output
            .as_ref()
            .and_then(|name| self.outputs.iter().position(|o| &o.name == name))
        else {
            return Task::none();
        };
        let note_x = note.x;
        let note_y = note.y;
        let note_width = note.width;
        let current_width = self.outputs[current_index].width;

        let at_left_edge = note_x <= 2;
        let at_right_edge = note_x + note_width >= current_width - 2;

        let target_index = if at_right_edge && current_index + 1 < self.outputs.len() {
            current_index + 1
        } else if at_left_edge && current_index > 0 {
            current_index - 1
        } else {
            return Task::none();
        };

        let moving_right = target_index > current_index;
        let target = self.outputs[target_index].clone();
        let new_x = if moving_right {
            8
        } else {
            (target.width - note_width - 8).max(0)
        };
        let new_y = note_y.clamp(
            0,
            (target.height - self.settings.size_preset.note_height() as i32).max(0),
        );

        let Some(note) = self.notes.get_mut(&note_id) else {
            return Task::none();
        };
        note.output = Some(target.name.clone());
        note.x = new_x;
        note.y = new_y;

        self.recreate_note_surface(note_id)
    }

    /// Closes a note's current surface (if it has one) and immediately
    /// creates a fresh one from its up-to-date fields. This is the only way
    /// to move a note to a different output, or make an `OutputOption`
    /// change take effect at all — both are fixed for the lifetime of a
    /// layer surface.
    fn recreate_note_surface(&mut self, note_id: u64) -> Task<Message> {
        let mut tasks = Vec::new();
        if let Some(old_surface) = self.note_surface.remove(&note_id) {
            self.surfaces.remove(&old_surface);
            tasks.push(Task::done(Message::RemoveWindow(old_surface)));
        }
        let Some(note) = self.notes.get(&note_id) else {
            return Task::batch(tasks);
        };
        let surface_id = window::Id::unique();
        self.surfaces.insert(surface_id, note_id);
        self.note_surface.insert(note_id, surface_id);
        let expanded = self.menu_open.contains(&note_id);
        let settings =
            note_layer_settings(note, expanded, &self.outputs, self.settings.size_preset);
        let margin = settings.margin.unwrap_or((note.y, 0, 0, note.x));
        tasks.push(Task::done(Message::NewLayerShell {
            settings,
            id: surface_id,
        }));
        // Nudge: re-commit the margin right after creation. Without it the
        // fresh surface sometimes stays unrendered until the next input
        // event reaches the app (observed as "note vanishes on monitor hop,
        // reappears when focus changes").
        tasks.push(Task::done(Message::MarginChange {
            id: surface_id,
            margin,
        }));
        Task::batch(tasks)
    }

    /// Same edge-hop judgement as `maybe_hop_output`, but for the
    /// "포스트잇 목록" panel: if `list_pos` is now flush against the left or
    /// right edge of its current output and an adjacent output exists in
    /// that direction, moves the panel there. Unlike notes, the panel always
    /// has a well-defined "current output" to hop from even before it's ever
    /// been explicitly placed (`list_output == None` is treated as output 0,
    /// matching the legacy-note migration rule in `PostitApp::new`). No-op
    /// if the panel isn't near an edge, or there's nothing to hop to in that
    /// direction. Position is not persisted, so nothing is saved here.
    fn maybe_hop_list_output(&mut self) -> Task<Message> {
        if self.outputs.len() < 2 {
            return Task::none();
        }

        let current_index = self
            .list_output
            .as_ref()
            .and_then(|name| self.outputs.iter().position(|o| &o.name == name))
            .unwrap_or(0);
        let (list_x, list_y) = self.list_pos;
        let current_width = self.outputs[current_index].width;

        let at_left_edge = list_x <= 2;
        let at_right_edge = list_x + LIST_SIZE.0 as i32 >= current_width - 2;

        let target_index = if at_right_edge && current_index + 1 < self.outputs.len() {
            current_index + 1
        } else if at_left_edge && current_index > 0 {
            current_index - 1
        } else {
            return Task::none();
        };

        let moving_right = target_index > current_index;
        let target = self.outputs[target_index].clone();
        let new_x = if moving_right {
            8
        } else {
            (target.width - LIST_SIZE.0 as i32 - 8).max(0)
        };
        let new_y = list_y.clamp(0, (target.height - LIST_SIZE.1 as i32).max(0));

        self.list_output = Some(target.name.clone());
        self.list_pos = (new_x, new_y);

        self.recreate_list_surface()
    }

    /// Closes the list panel's current surface (if open) and immediately
    /// creates a fresh one from `list_output`/`list_pos`. Mirrors
    /// `recreate_note_surface` — layer surfaces can't change `wl_output` or
    /// `OutputOption` in place. No-op if the panel is currently closed.
    fn recreate_list_surface(&mut self) -> Task<Message> {
        let Some(old_surface) = self.list_surface.take() else {
            return Task::none();
        };
        // Note: list surfaces are never registered in `self.surfaces` (that
        // map is note-only — see `ToggleList`), so there's nothing else to
        // remove here.
        let mut tasks = vec![Task::done(Message::RemoveWindow(old_surface))];

        let id = window::Id::unique();
        self.list_surface = Some(id);
        let settings = list_layer_settings(&self.list_output, &self.outputs, self.list_pos);
        let margin = settings
            .margin
            .unwrap_or((self.list_pos.1, 0, 0, self.list_pos.0));
        tasks.push(Task::done(Message::NewLayerShell { settings, id }));
        // Nudge: re-commit the margin right after creation, same reason as
        // `recreate_note_surface`'s Nudge comment.
        tasks.push(Task::done(Message::MarginChange { id, margin }));
        Task::batch(tasks)
    }

    fn handle_active_app_changed(&mut self, app_id: String) -> Task<Message> {
        // Editing/clicking on postit's own surfaces must not disturb the
        // visibility judgement made against the previously active app.
        if app_id.is_empty() || app_id == SELF_APP_ID {
            return Task::none();
        }
        if self.active_app.as_deref() == Some(app_id.as_str()) {
            return Task::none();
        }
        self.active_app = Some(app_id);
        self.reconcile_visibility()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CreateNote(color) => {
                let id = new_note_id();
                let preset = self.settings.size_preset;
                // Cascade rightward only, all notes sharing the first note's
                // y — not the old diagonal down-right cascade — so a run of
                // freshly-created notes reads as one row instead of a
                // staircase. Stepping by the note's own width (plus a small
                // gap) rather than a fixed pixel amount keeps consecutive
                // notes from overlapping regardless of size preset.
                const CASCADE_GAP: i32 = 12;
                let step = ((self.notes.len() as i32) % 10) * (preset.default_note_width() + CASCADE_GAP);
                let x = 120 + step;
                let y = 120;
                let bound_app = self.active_app.clone();
                let mut note = Note::new(id, color, x, y, bound_app);
                note.width = preset.default_note_width();
                if let Some(first) = self.outputs.first() {
                    note.output = Some(first.name.clone());
                }

                let surface_id = window::Id::unique();
                self.surfaces.insert(surface_id, id);
                self.note_surface.insert(id, surface_id);
                let settings =
                    note_layer_settings(&note, false, &self.outputs, self.settings.size_preset);
                self.notes.insert(id, note);
                self.save();

                let new_layer_task = Task::done(Message::NewLayerShell {
                    settings,
                    id: surface_id,
                });
                let focus_task = iced::widget::operation::focus::<Message>(format!("postit-input-{}", id));
                Task::batch(vec![new_layer_task, focus_task])
            }
            Message::TextChanged(note_id, text) => {
                let Some(note) = self.notes.get(&note_id) else {
                    return Task::none();
                };
                // `text_input` fires `on_input` unconditionally on every
                // Delete/Backspace press — even one with nothing to delete —
                // republishing its (possibly unchanged) contents. So an
                // already-empty field publishing empty again is exactly a
                // Delete/Backspace pressed with nothing left to remove,
                // distinct from selecting existing text and clearing it
                // (which goes non-empty -> empty, handled below like any
                // other edit). Treat that no-op case as "delete this note"
                // instead of Del conflicting with clear-and-retype.
                if note.text.is_empty() && text.is_empty() {
                    return self.update(Message::DeleteNote(note_id));
                }

                let preset = self.settings.size_preset;
                let mut resize_task = Task::none();
                if let Some(note) = self.notes.get_mut(&note_id) {
                    note.text = text;
                    // Grow-to-fit, but only rightward and only up to
                    // `MAX_NOTE_WIDTH` — the same ceiling the resize handle
                    // is clamped to. Never shrinks back on deletion; that
                    // stays the resize handle's job.
                    let desired = preset
                        .text_width_estimate(&note.text)
                        .clamp(preset.default_note_width(), MAX_NOTE_WIDTH);
                    if desired > note.width {
                        note.width = desired;
                        if let Some(&surface_id) = self.note_surface.get(&note_id) {
                            let height = if self.menu_open.contains(&note_id) {
                                preset.note_expanded_height()
                            } else {
                                preset.note_height()
                            };
                            resize_task = Task::done(Message::SizeChange {
                                id: surface_id,
                                size: (note.width as u32, height),
                            });
                        }
                    }
                }
                self.save();
                resize_task
            }
            Message::ToggleMenu(note_id) => {
                let Some(&surface_id) = self.note_surface.get(&note_id) else {
                    return Task::none();
                };
                let expanded = if self.menu_open.contains(&note_id) {
                    self.menu_open.remove(&note_id);
                    false
                } else {
                    self.menu_open.insert(note_id);
                    true
                };
                let preset = self.settings.size_preset;
                // The width must stay at the note's own (possibly
                // user-resized) width, not the default — except that
                // expanding the menu floors it at `min_note_width()` so the
                // options row's icons are never clipped; collapsing restores
                // the note's actual stored width.
                let note_width = self
                    .notes
                    .get(&note_id)
                    .map(|note| note.width)
                    .unwrap_or(preset.default_note_width());
                let width = if expanded {
                    note_width.max(preset.min_note_width())
                } else {
                    note_width
                } as u32;
                let height = if expanded {
                    preset.note_expanded_height()
                } else {
                    preset.note_height()
                };
                Task::done(Message::SizeChange {
                    id: surface_id,
                    size: (width, height),
                })
            }
            Message::ColorChanged(note_id, color) => {
                if let Some(note) = self.notes.get_mut(&note_id) {
                    note.color = color;
                }
                self.save();

                let mut tasks = Vec::new();

                if let Some(&surface_id) = self.note_surface.get(&note_id) {
                    if self.menu_open.remove(&note_id) {
                        // Menu was open, close it and adjust size back down to
                        // the note's own stored width (no min-width floor —
                        // that only applies while the menu is expanded).
                        let preset = self.settings.size_preset;
                        let height = preset.note_height();
                        let width = self
                            .notes
                            .get(&note_id)
                            .map(|note| note.width as u32)
                            .unwrap_or(preset.default_note_width() as u32);
                        tasks.push(Task::done(Message::SizeChange {
                            id: surface_id,
                            size: (width, height),
                        }));
                    }
                }

                if tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(tasks)
                }
            }
            Message::ToggleAlwaysVisible(note_id) => {
                if let Some(note) = self.notes.get_mut(&note_id) {
                    note.always_visible = !note.always_visible;
                }
                self.save();

                let mut tasks = Vec::new();

                // Close menu if open
                if let Some(&surface_id) = self.note_surface.get(&note_id) {
                    if self.menu_open.remove(&note_id) {
                        // Menu was open, close it and adjust size back down to
                        // the note's own stored width (no min-width floor —
                        // that only applies while the menu is expanded).
                        let preset = self.settings.size_preset;
                        let height = preset.note_height();
                        let width = self
                            .notes
                            .get(&note_id)
                            .map(|note| note.width as u32)
                            .unwrap_or(preset.default_note_width() as u32);
                        tasks.push(Task::done(Message::SizeChange {
                            id: surface_id,
                            size: (width, height),
                        }));
                    }
                }

                tasks.push(self.reconcile_visibility());
                Task::batch(tasks)
            }
            Message::DeleteNote(note_id) => {
                self.menu_open.remove(&note_id);
                self.notes.remove(&note_id);
                let task = if let Some(surface_id) = self.note_surface.remove(&note_id) {
                    self.surfaces.remove(&surface_id);
                    Task::done(Message::RemoveWindow(surface_id))
                } else {
                    Task::none()
                };
                self.save();
                task
            }
            Message::MoveToNextOutput(note_id) => {
                if self.outputs.len() < 2 || !self.notes.contains_key(&note_id) {
                    return Task::none();
                }

                // Remove from menu_open before recreating surface, so it recreates in collapsed state
                self.menu_open.remove(&note_id);

                let current_index = self.notes[&note_id]
                    .output
                    .as_ref()
                    .and_then(|name| self.outputs.iter().position(|o| &o.name == name))
                    .unwrap_or(0);
                let next_name = self.outputs[(current_index + 1) % self.outputs.len()].name.clone();

                if let Some(note) = self.notes.get_mut(&note_id) {
                    note.output = Some(next_name);
                    note.x = 160;
                    note.y = 160;
                }

                let task = self.recreate_note_surface(note_id);
                self.save();
                task
            }
            Message::RebindApp(note_id) => {
                if let Some(note) = self.notes.get_mut(&note_id) {
                    note.bound_app = self.active_app.clone();
                }
                self.save();

                let mut tasks = Vec::new();

                // Close menu if open
                if let Some(&surface_id) = self.note_surface.get(&note_id) {
                    if self.menu_open.remove(&note_id) {
                        // Menu was open, close it and adjust size back down to
                        // the note's own stored width (no min-width floor —
                        // that only applies while the menu is expanded).
                        let preset = self.settings.size_preset;
                        let height = preset.note_height();
                        let width = self
                            .notes
                            .get(&note_id)
                            .map(|note| note.width as u32)
                            .unwrap_or(preset.default_note_width() as u32);
                        tasks.push(Task::done(Message::SizeChange {
                            id: surface_id,
                            size: (width, height),
                        }));
                    }
                }

                tasks.push(self.reconcile_visibility());
                Task::batch(tasks)
            }
            Message::DragStart(note_id) => {
                if let (Some(&surface_id), Some(note)) =
                    (self.note_surface.get(&note_id), self.notes.get(&note_id))
                {
                    // Mutually exclusive with resizing: starting a drag
                    // abandons any in-progress resize (and vice versa below).
                    self.resize = None;
                    self.drag = Some(DragState {
                        target: DragTarget::Note(note_id),
                        surface_id,
                        start: (note.x, note.y),
                        press: None,
                        last_apply: None,
                    });
                }
                Task::none()
            }
            Message::ResizeStart(note_id) => {
                if let (Some(&surface_id), Some(note)) =
                    (self.note_surface.get(&note_id), self.notes.get(&note_id))
                {
                    self.drag = None;
                    self.resize = Some(ResizeState {
                        note_id,
                        surface_id,
                        start_width: note.width,
                        press: None,
                        last_apply: None,
                    });
                }
                Task::none()
            }
            Message::WindowClosed(id) => {
                if let Some(note_id) = self.surfaces.remove(&id) {
                    if self.note_surface.get(&note_id) == Some(&id) {
                        self.note_surface.remove(&note_id);
                    }
                }
                if self.list_surface == Some(id) {
                    self.list_surface = None;
                }
                if self.toolbar_surface == Some(id) {
                    self.toolbar_surface = None;
                }
                Task::none()
            }
            Message::IcedEvent(id, event) => self.handle_iced_event(id, event),
            Message::ActiveAppChanged(app_id) => self.handle_active_app_changed(app_id),
            Message::TrayMessage(msg) => self.handle_tray_message(msg),
            Message::ToggleList => {
                if let Some(id) = self.list_surface.take() {
                    Task::done(Message::RemoveWindow(id))
                } else {
                    let id = window::Id::unique();
                    self.list_surface = Some(id);
                    Task::done(Message::NewLayerShell {
                        settings: list_layer_settings(&self.list_output, &self.outputs, self.list_pos),
                        id,
                    })
                }
            }
            Message::MoveListToNextOutput => {
                if self.outputs.len() < 2 {
                    return Task::none();
                }

                let current_index = self.list_output
                    .as_ref()
                    .and_then(|name| self.outputs.iter().position(|o| &o.name == name))
                    .unwrap_or(0);
                let next_name = self.outputs[(current_index + 1) % self.outputs.len()].name.clone();
                self.list_output = Some(next_name);
                // Reset to the default position on every explicit
                // next-output hop, same as notes reset to (160, 160) in
                // `MoveToNextOutput` — a position dragged near an edge on the
                // old output would otherwise often land off-screen or
                // immediately re-hop on the new one.
                self.list_pos = LIST_DEFAULT_POS;

                self.recreate_list_surface()
            }
            Message::ListDragStart => {
                if let Some(surface_id) = self.list_surface {
                    // Mutually exclusive with note resizing/dragging, same
                    // rule as `Message::DragStart` for notes.
                    self.resize = None;
                    self.drag = Some(DragState {
                        target: DragTarget::List,
                        surface_id,
                        start: self.list_pos,
                        press: None,
                        last_apply: None,
                    });
                }
                Task::none()
            }
            Message::ImportNote(note_id) => {
                if !self.notes.contains_key(&note_id) {
                    return Task::none();
                }
                // Cascade new positions by how many note surfaces are
                // currently on screen, same idea as `CreateNote`.
                let step = (self.note_surface.len() as i32) * 24;
                let x = 160 + step;
                let y = 160 + step;
                let has_surface = self.note_surface.contains_key(&note_id);
                if let Some(note) = self.notes.get_mut(&note_id) {
                    note.x = x;
                    note.y = y;
                    // Only re-home a note that has no live surface: one that
                    // does is still physically shown on whatever output it
                    // was created against (a layer surface's output can't
                    // change without a close/recreate), so overwriting
                    // `note.output` here would desync the recorded output
                    // from where the note actually is. A note with no
                    // surface is about to get a brand-new one anyway, so
                    // this is the same "surface-creation-time" rule as
                    // legacy-note migration (plan 8.3 point 3/5).
                    if !has_surface {
                        if let Some(first) = self.outputs.first() {
                            note.output = Some(first.name.clone());
                        }
                    }
                }
                self.save();

                let margin = (y, 0, 0, x);
                if let Some(&surface_id) = self.note_surface.get(&note_id) {
                    Task::done(Message::MarginChange {
                        id: surface_id,
                        margin,
                    })
                } else {
                    let surface_id = window::Id::unique();
                    self.surfaces.insert(surface_id, note_id);
                    self.note_surface.insert(note_id, surface_id);
                    let expanded = self.menu_open.contains(&note_id);
                    let settings = note_layer_settings(
                        &self.notes[&note_id],
                        expanded,
                        &self.outputs,
                        self.settings.size_preset,
                    );
                    Task::done(Message::NewLayerShell {
                        settings,
                        id: surface_id,
                    })
                }
            }
            Message::SetSizePreset(preset) => {
                self.settings.size_preset = preset;
                settings::save_settings(&self.settings);

                // Resize every currently-open note surface to the new
                // preset's heights (and, if its menu is open, the min-width
                // floor too) — the list panel is intentionally left alone.
                let mut tasks = Vec::new();
                for (&note_id, &surface_id) in self.note_surface.iter() {
                    let expanded = self.menu_open.contains(&note_id);
                    let Some(note) = self.notes.get(&note_id) else {
                        continue;
                    };
                    let width = if expanded {
                        note.width.max(preset.min_note_width())
                    } else {
                        note.width
                    } as u32;
                    let height = if expanded {
                        preset.note_expanded_height()
                    } else {
                        preset.note_height()
                    };
                    tasks.push(Task::done(Message::SizeChange {
                        id: surface_id,
                        size: (width, height),
                    }));
                }
                Task::batch(tasks)
            }
            Message::SetOpacity(opacity) => {
                self.settings.opacity = opacity;
                settings::save_settings(&self.settings);
                Task::none()
            }
            Message::Quit => {
                self.save();
                iced::exit()
            }
            _ => Task::none(),
        }
    }

    /// Handles a message forwarded from the tray thread (`tray.rs`): the
    /// initial registration outcome, or a menu/activation event.
    fn handle_tray_message(&mut self, msg: TrayMessage) -> Task<Message> {
        match msg {
            TrayMessage::Registered => Task::none(),
            TrayMessage::Unavailable => {
                if self.toolbar_surface.is_some() {
                    return Task::none();
                }
                let id = window::Id::unique();
                self.toolbar_surface = Some(id);
                Task::done(Message::NewLayerShell {
                    settings: toolbar_layer_settings(),
                    id,
                })
            }
            TrayMessage::Event(TrayEvent::NewNote(color)) => {
                self.update(Message::CreateNote(color))
            }
            TrayMessage::Event(TrayEvent::RefreshOutputs) => {
                self.refresh_outputs();
                Task::none()
            }
            TrayMessage::Event(TrayEvent::GatherNotes) => {
                self.gather_all_notes()
            }
            TrayMessage::Event(TrayEvent::ShowList) => self.update(Message::ToggleList),
            TrayMessage::Event(TrayEvent::SetSizePreset(preset)) => {
                self.update(Message::SetSizePreset(preset))
            }
            TrayMessage::Event(TrayEvent::SetOpacity(opacity)) => {
                self.update(Message::SetOpacity(opacity))
            }
            TrayMessage::Event(TrayEvent::Quit) => self.update(Message::Quit),
        }
    }

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        if Some(id) == self.list_surface {
            return list_view::view(&self.notes);
        }
        if let Some(&note_id) = self.surfaces.get(&id) {
            if let Some(note) = self.notes.get(&note_id) {
                let expanded = self.menu_open.contains(&note_id);
                return note_view::view(note, expanded, &self.settings);
            }
        }
        toolbar::view()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            event::listen_with(|event, status, id| match event {
                Event::Mouse(_) => Some(Message::IcedEvent(id, event)),
                // Only when no widget claimed it — a focused `text_input`
                // always captures Delete/Backspace itself (see
                // `TextChanged`'s own empty-check), so this only ever fires
                // when the note's grip/resize-handle/menu was clicked
                // instead of its text field, which leaves no widget focused
                // to catch the key.
                Event::Keyboard(_) if status == event::Status::Ignored => {
                    Some(Message::IcedEvent(id, event))
                }
                _ => None,
            }),
            window::close_events().map(Message::WindowClosed),
            Subscription::run(focus_stream),
            Subscription::run(tray_stream),
        ])
    }
}

/// Bridges the synchronous focus-tracking thread (see `focus.rs`) into an
/// async iced `Stream`. Spawns a tiny forwarding thread that turns blocking
/// `std::sync::mpsc::Receiver::recv` calls into non-blocking pushes onto a
/// `futures` unbounded channel, so nothing here ever blocks iced's executor.
fn focus_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::StreamExt;

    let rx = focus::spawn();
    let (async_tx, async_rx) = iced::futures::channel::mpsc::unbounded();

    std::thread::spawn(move || {
        while let Ok(app_id) = rx.recv() {
            if async_tx.unbounded_send(app_id).is_err() {
                break;
            }
        }
    });

    async_rx.map(Message::ActiveAppChanged)
}

/// Bridges the tray thread (see `tray.rs`) into an async iced `Stream`, same
/// pattern as `focus_stream` above.
fn tray_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::StreamExt;

    let rx = tray::spawn();
    let (async_tx, async_rx) = iced::futures::channel::mpsc::unbounded();

    std::thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            if async_tx.unbounded_send(msg).is_err() {
                break;
            }
        }
    });

    async_rx.map(Message::TrayMessage)
}

pub fn run() -> iced_layershell::Result {
    daemon(
        PostitApp::new,
        PostitApp::namespace,
        PostitApp::update,
        PostitApp::view,
    )
    .subscription(PostitApp::subscription)
    .settings(Settings {
        id: Some(SELF_APP_ID.to_string()),
        layer_settings: LayerShellSettings {
            anchor: Anchor::Top,
            layer: Layer::Top,
            // No initial surface: we start in the background and only ever
            // create the fallback toolbar (see `handle_tray_message`) if the
            // tray icon fails to register. This needs no `size`, which is
            // exactly what `StartMode::Background` allows (see the `assert!`
            // in iced_layershell's `Daemon::run`).
            size: None,
            margin: (0, 0, 0, 0),
            exclusive_zone: 0,
            keyboard_interactivity: KeyboardInteractivity::None,
            start_mode: StartMode::Background,
            events_transparent: false,
        },
        ..Default::default()
    })
    .run()
}
