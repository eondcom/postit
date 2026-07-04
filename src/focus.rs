//! Active-window (app_id) tracking.
//!
//! Runs the Wayland event loop on its own OS thread and reports the app_id of
//! the currently activated toplevel over a `std::sync::mpsc` channel whenever
//! it changes.
//!
//! Two backends are supported, tried in this order:
//! 1. `zwlr_foreign_toplevel_manager_v1` (wlroots-family compositors).
//! 2. `zcosmic_toplevel_info_v1`, bound at **version 1** (COSMIC). COSMIC does
//!    not implement the zwlr protocol at all (confirmed by registry
//!    inspection) but does advertise this one, plus `ext_foreign_toplevel_list_v1`
//!    which isn't used here. Binding v1 (even though the compositor may
//!    advertise up to v3) keeps the wire semantics identical to the zwlr
//!    backend: a `toplevel` event on the manager hands out a handle, and the
//!    handle reports `app_id`/`state` events terminated by `done`. See
//!    `protocols/cosmic-toplevel-info-unstable-v1.xml` for the (locally
//!    trimmed) protocol definition this is generated from.
//!
//! If neither protocol is advertised, a warning is printed and the thread
//! exits immediately (callers should fall back to treating every note as
//! always-visible).

use std::collections::HashMap;
use std::error::Error;
use std::sync::mpsc::Sender;
use std::thread;

use wayland_client::backend::ObjectId;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use cosmic_protocol::zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1};
use cosmic_protocol::zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1};

const ACTIVATED_STATE: u32 = 2;

/// Generated bindings for `zcosmic_toplevel_info_v1` / `zcosmic_toplevel_handle_v1`,
/// produced at compile time by `wayland-scanner`'s client-code macros from
/// the local XML copy in `protocols/`. See that file's header comment for
/// the (safe, v1-only) edits made to it so codegen resolves without pulling
/// in the rest of the cosmic-protocols ecosystem.
#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    unused_imports,
    dead_code,
    missing_docs,
    clippy::all
)]
mod cosmic_protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/cosmic-toplevel-info-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/cosmic-toplevel-info-unstable-v1.xml");
}

#[derive(Default, Clone)]
struct ToplevelInfo {
    app_id: String,
    activated: bool,
    // Staged values, applied atomically on `done`.
    pending_app_id: Option<String>,
    pending_activated: Option<bool>,
}

struct FocusState {
    /// Populated only when the zwlr backend is the one in use.
    zwlr_toplevels: HashMap<ObjectId, ToplevelInfo>,
    /// Populated only when the zcosmic backend is the one in use.
    cosmic_toplevels: HashMap<ObjectId, ToplevelInfo>,
    last_sent: Option<String>,
    tx: Sender<String>,
}

impl FocusState {
    fn recompute_and_send(&mut self) {
        let active_app_id = self
            .zwlr_toplevels
            .values()
            .find(|info| info.activated)
            .or_else(|| self.cosmic_toplevels.values().find(|info| info.activated))
            .map(|info| info.app_id.clone());

        if active_app_id != self.last_sent {
            if let Some(app_id) = active_app_id.clone() {
                // Ignore send errors: the receiving end (iced subscription)
                // may have gone away during shutdown.
                let _ = self.tx.send(app_id);
            }
            self.last_sent = active_app_id;
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for FocusState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // Dynamic global add/remove is not needed: we only care about the
        // toplevel manager, which is bound once at startup.
    }
}

// --- zwlr_foreign_toplevel_manager_v1 backend -------------------------------

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for FocusState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state
                .zwlr_toplevels
                .insert(toplevel.id(), ToplevelInfo::default());
        }
        // `finished` (manager destroyed/replaced) is ignored; the process
        // lives for the lifetime of the app.
    }

    wayland_client::event_created_child!(FocusState, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for FocusState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let id = proxy.id();
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(info) = state.zwlr_toplevels.get_mut(&id) {
                    info.pending_app_id = Some(app_id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: raw_state } => {
                let activated = raw_state
                    .chunks_exact(4)
                    .any(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) == ACTIVATED_STATE);
                if let Some(info) = state.zwlr_toplevels.get_mut(&id) {
                    info.pending_activated = Some(activated);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(info) = state.zwlr_toplevels.get_mut(&id) {
                    if let Some(app_id) = info.pending_app_id.take() {
                        info.app_id = app_id;
                    }
                    if let Some(activated) = info.pending_activated.take() {
                        info.activated = activated;
                    }
                }
                state.recompute_and_send();
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.zwlr_toplevels.remove(&id);
                state.recompute_and_send();
            }
            _ => {}
        }
    }
}

// --- zcosmic_toplevel_info_v1 backend (COSMIC), bound at version 1 --------
//
// v1 semantics mirror the zwlr protocol closely: `toplevel` hands out a
// handle, and the handle reports `app_id`/`state` (same activated-bit array
// encoding) terminated by `done`. Several event/enum variants below carry a
// `#[deprecated(since = "2")]`-style attribute in the generated code (they
// are documented as v1-only, replaced by `ext_foreign_toplevel_*` events for
// v2+ clients) — irrelevant here since we deliberately bind v1, hence the
// blanket `allow(deprecated)`.

#[allow(deprecated)]
impl Dispatch<ZcosmicToplevelInfoV1, ()> for FocusState {
    fn event(
        state: &mut Self,
        _proxy: &ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Toplevel { toplevel } = event {
            state
                .cosmic_toplevels
                .insert(toplevel.id(), ToplevelInfo::default());
        }
        // `finished` (manager destroyed/replaced) is ignored; the process
        // lives for the lifetime of the app.
    }

    wayland_client::event_created_child!(FocusState, ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (ZcosmicToplevelHandleV1, ()),
    ]);
}

#[allow(deprecated)]
impl Dispatch<ZcosmicToplevelHandleV1, ()> for FocusState {
    fn event(
        state: &mut Self,
        proxy: &ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let id = proxy.id();
        match event {
            zcosmic_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(info) = state.cosmic_toplevels.get_mut(&id) {
                    info.pending_app_id = Some(app_id);
                }
            }
            zcosmic_toplevel_handle_v1::Event::State { state: raw_state } => {
                let activated = raw_state
                    .chunks_exact(4)
                    .any(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) == ACTIVATED_STATE);
                if let Some(info) = state.cosmic_toplevels.get_mut(&id) {
                    info.pending_activated = Some(activated);
                }
            }
            zcosmic_toplevel_handle_v1::Event::Done => {
                if let Some(info) = state.cosmic_toplevels.get_mut(&id) {
                    if let Some(app_id) = info.pending_app_id.take() {
                        info.app_id = app_id;
                    }
                    if let Some(activated) = info.pending_activated.take() {
                        info.activated = activated;
                    }
                }
                state.recompute_and_send();
            }
            zcosmic_toplevel_handle_v1::Event::Closed => {
                state.cosmic_toplevels.remove(&id);
                state.recompute_and_send();
            }
            _ => {}
        }
    }
}

/// Spawns the focus-tracking thread and returns the receiving end of the
/// channel it reports active app_ids on. If the compositor doesn't support
/// either supported protocol, a warning is printed and the thread exits
/// after sending nothing, in which case the channel simply never yields
/// anything (callers should treat that as "no information available" and
/// keep every note visible).
pub fn spawn() -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        if let Err(err) = run(tx) {
            eprintln!(
                "[postit] focus tracking disabled ({err}); all notes will stay visible"
            );
        }
    });
    rx
}

fn run(tx: Sender<String>) -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<FocusState>(&conn)?;
    let qh = queue.handle();

    let mut state = FocusState {
        zwlr_toplevels: HashMap::new(),
        cosmic_toplevels: HashMap::new(),
        last_sent: None,
        tx,
    };

    // Priority 1: zwlr_foreign_toplevel_manager_v1 (wlroots-family
    // compositors). COSMIC does not advertise this at all.
    if let Ok(manager) = globals.bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=1, ()) {
        eprintln!("[postit] focus tracking: zwlr_foreign_toplevel_manager_v1 backend");
        loop {
            queue.blocking_dispatch(&mut state)?;
            // Keep the binding alive for the lifetime of the loop; its Drop
            // is what would tear down the binding, which we never want
            // while polling.
            let _ = &manager;
        }
    }

    // Priority 2: zcosmic_toplevel_info_v1, bound at v1 even though COSMIC
    // may advertise up to v3 (see module docs).
    if let Ok(manager) = globals.bind::<ZcosmicToplevelInfoV1, _, _>(&qh, 1..=1, ()) {
        eprintln!("[postit] focus tracking: zcosmic_toplevel_info_v1 backend (bound v1)");
        loop {
            queue.blocking_dispatch(&mut state)?;
            let _ = &manager;
        }
    }

    Err("compositor advertises neither zwlr_foreign_toplevel_manager_v1 nor zcosmic_toplevel_info_v1"
        .into())
}
