//! `openstream-testkit` — deterministic test infrastructure shared by crates.
//!
//! M2: golden-fixture loader and fake-clock helpers for codec contract tests.

use std::path::Path;

/// Load a JSON fixture file and return its raw contents.
pub fn load_fixture<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    std::fs::read_to_string(path.as_ref())
}
