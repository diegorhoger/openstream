//! OpenStream desktop composition root.
//!
//! This binary only *composes*: it boots the Tauri shell that hosts the
//! Studio UI. All product authority (validation, permissions, execution,
//! persistence) lives in the Rust workspace crates per ADR-0001. No Tauri
//! IPC commands are registered at this stage, so the WebView has no
//! invokable surface; capabilities stay deny-by-default.

// Hide the console window in release builds; the shell is a desktop app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("OpenStream desktop shell failed to start");
}
