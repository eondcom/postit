//! Registers only the tray icon (no Wayland/GUI) so SNI registration and
//! panel rendering can be verified independently:
//! `cargo run --example tray_probe`

#[path = "../src/colors.rs"]
mod colors;
#[path = "../src/tray.rs"]
mod tray;

fn main() {
    let rx = tray::spawn();
    println!("[tray_probe] waiting for tray messages (Ctrl+C to quit)");
    for msg in rx {
        println!("[tray_probe] {msg:?}");
    }
}
