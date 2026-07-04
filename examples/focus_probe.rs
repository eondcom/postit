//! Registers only the active-window tracker (no GUI) so the zwlr/zcosmic
//! backend selection and app_id reporting in `focus.rs` can be verified
//! independently: `cargo run --example focus_probe`
//!
//! Prints each active app_id as it changes. On a compositor supporting
//! neither protocol, `focus.rs` logs a warning to stderr and this prints
//! nothing (the channel never yields), matching how `app.rs` falls back to
//! "always visible".

#[path = "../src/focus.rs"]
mod focus;

fn main() {
    let rx = focus::spawn();
    println!("[focus_probe] waiting for active app_id changes (Ctrl+C to quit)");
    for app_id in rx {
        println!("[focus_probe] active app_id: {app_id}");
    }
}
