use crate::note::Note;
use std::fs;
use std::path::PathBuf;

fn get_notes_path() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("postit").join("notes.json")
    } else {
        PathBuf::from(".local/share/postit/notes.json")
    }
}

pub fn load_notes() -> Vec<Note> {
    let path = get_notes_path();

    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Vec<Note>>(&content) {
            Ok(notes) => notes,
            Err(e) => {
                eprintln!("Failed to parse notes.json: {}", e);
                Vec::new()
            }
        },
        Err(_) => {
            // File not found or read error; return empty vec
            Vec::new()
        }
    }
}

pub fn save_notes(notes: &[Note]) {
    let path = get_notes_path();

    // Create parent directory
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("Failed to create directory for notes: {}", e);
            return;
        }
    }

    // Write to temporary file first
    let tmp_path = path.with_file_name("notes.json.tmp");

    let json_string = match serde_json::to_string_pretty(notes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize notes: {}", e);
            return;
        }
    };

    if let Err(e) = fs::write(&tmp_path, json_string) {
        eprintln!("Failed to write temporary notes file: {}", e);
        return;
    }

    // Atomically rename
    if let Err(e) = fs::rename(&tmp_path, &path) {
        eprintln!("Failed to rename notes file: {}", e);
        // Clean up tmp file on failure
        let _ = fs::remove_file(&tmp_path);
    }
}
