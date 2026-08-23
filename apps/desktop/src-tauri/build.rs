//! Build script for the OpenStream desktop composition root.
//!
//! Invokes `tauri_build` to generate the Tauri context code and capability
//! schemas from `tauri.conf.json` and the `capabilities/` directory.

fn main() {
    tauri_build::build();
}
