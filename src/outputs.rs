//! One-shot output (monitor) enumeration via `zxdg_output_manager_v1`.
//!
//! Unlike `focus.rs`, this does not run a background thread: it opens its own
//! short-lived Wayland connection, does two roundtrips to collect every
//! output's logical name/position/size, and closes. Called once at startup
//! (see `app.rs`), not kept alive — layer surfaces are created against
//! `PostitApp::outputs` afterwards, not against this connection.

use std::collections::HashMap;
use std::error::Error;

use wayland_client::backend::ObjectId;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::{self, ZxdgOutputManagerV1},
    zxdg_output_v1::{self, ZxdgOutputV1},
};

/// One display, as reported by `zxdg_output_manager_v1`. `name` is the
/// compositor's logical output name (e.g. "DP-1"), which is what
/// `OutputOption::OutputName` and `Note::output` key off of.
#[derive(Clone, Debug)]
pub struct OutputInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Accumulates the events for one output until `done` fires, since
/// `zxdg_output_v1` reports name/logical_position/logical_size as separate
/// events that only become a coherent snapshot once `done` arrives.
#[derive(Default)]
struct PendingOutput {
    name: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct OutputsState {
    // Keyed by the wl_output's ObjectId, which we pass through as the
    // xdg_output's user data so `done` knows which entry to finalize.
    pending: HashMap<ObjectId, PendingOutput>,
    results: Vec<OutputInfo>,
}

impl OutputsState {
    fn finalize(&mut self, id: &ObjectId) {
        if let Some(pending) = self.pending.remove(id) {
            self.results.push(OutputInfo {
                name: pending.name,
                x: pending.x,
                y: pending.y,
                width: pending.width,
                height: pending.height,
            });
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for OutputsState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // One-shot enumeration: the process exits right after, so dynamic
        // global add/remove doesn't matter.
    }
}

impl Dispatch<WlOutput, ()> for OutputsState {
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // xdg_output v3 deprecates its own `done` event: compositors (COSMIC
        // included) stop sending it and atomicity moves to wl_output.done,
        // so the snapshot must be finalized here as well.
        if let wl_output::Event::Done = event {
            state.finalize(&proxy.id());
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for OutputsState {
    fn event(
        _state: &mut Self,
        _proxy: &ZxdgOutputManagerV1,
        _event: zxdg_output_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // zxdg_output_manager_v1 has no events.
    }
}

impl Dispatch<ZxdgOutputV1, ObjectId> for OutputsState {
    fn event(
        state: &mut Self,
        _proxy: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zxdg_output_v1::Event::Name { name } => {
                state.pending.entry(data.clone()).or_default().name = name;
            }
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                let entry = state.pending.entry(data.clone()).or_default();
                entry.x = x;
                entry.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                let entry = state.pending.entry(data.clone()).or_default();
                entry.width = width;
                entry.height = height;
            }
            // Only sent by xdg_output v1/v2 servers; on v3 the finalize
            // happens in the wl_output.done handler instead.
            zxdg_output_v1::Event::Done => {
                state.finalize(data);
            }
            _ => {}
        }
    }
}

/// Enumerates the compositor's outputs, sorted left-to-right by logical x
/// position. Returns an empty vector (after logging why) on any failure —
/// callers must treat that the same as "only one, unnamed output": fall back
/// to `OutputOption::None` and let the compositor pick.
pub fn list_outputs() -> Vec<OutputInfo> {
    match try_list_outputs() {
        Ok(outputs) => outputs,
        Err(err) => {
            eprintln!(
                "[postit] failed to enumerate outputs ({err}); falling back to compositor-chosen placement"
            );
            Vec::new()
        }
    }
}

fn try_list_outputs() -> Result<Vec<OutputInfo>, Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<OutputsState>(&conn)?;
    let qh = queue.handle();

    let manager: ZxdgOutputManagerV1 = globals
        .bind(&qh, 1..=3, ())
        .map_err(|_| "compositor does not advertise zxdg_output_manager_v1")?;

    let output_globals: Vec<(u32, u32)> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == "wl_output")
            .map(|g| (g.name, g.version))
            .collect()
    });

    let mut state = OutputsState {
        pending: HashMap::new(),
        results: Vec::new(),
    };

    let max_output_version = WlOutput::interface().version;
    for (name, version) in output_globals {
        let output: WlOutput = globals
            .registry()
            .bind(name, version.min(max_output_version), &qh, ());
        let output_id = output.id();
        manager.get_xdg_output(&output, &qh, output_id);
    }

    // First roundtrip lets the compositor process the get_xdg_output
    // requests just queued above; the second collects the resulting
    // name/logical_position/logical_size/done event burst for every output.
    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;

    // Safety net: if neither done event fired (protocol quirk), whatever has
    // accumulated a name by now is still a usable snapshot.
    let leftovers: Vec<ObjectId> = state.pending.keys().cloned().collect();
    for id in leftovers {
        if !state.pending[&id].name.is_empty() {
            state.finalize(&id);
        }
    }

    state.results.sort_by(|a, b| a.x.cmp(&b.x));
    Ok(state.results)
}
