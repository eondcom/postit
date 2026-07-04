mod app;
mod colors;
mod focus;
mod list_view;
mod note;
mod note_view;
mod outputs;
mod settings;
mod storage;
mod toolbar;
mod tray;

fn main() -> iced_layershell::Result {
    app::run()
}
