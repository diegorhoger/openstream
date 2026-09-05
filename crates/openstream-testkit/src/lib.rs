//! `openstream-testkit` — deterministic test infrastructure shared by crates.
//!
//! M2: golden-fixture loader and fake-clock helpers for codec contract tests.

use std::path::Path;

/// Load a JSON fixture file and return its raw contents.
pub fn load_fixture<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    std::fs::read_to_string(path.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Loading an existing readable file returns its bytes.
    #[test]
    fn load_fixture_reads_existing_file() {
        let dir = tempdir_unique();
        let path = dir.join("fixture.json");
        {
            let mut f = std::fs::File::create(&path).expect("create temp fixture");
            f.write_all(b"{\"a\":1}").expect("write");
        }
        let contents = load_fixture(&path).expect("read");
        assert_eq!(contents, "{\"a\":1}");
    }

    /// Loading a missing file returns an error rather than panicking.
    #[test]
    fn load_fixture_missing_file_errors() {
        let dir = tempdir_unique();
        let path = dir.join("does_not_exist.json");
        let result = load_fixture(&path);
        assert!(result.is_err());
    }

    /// The path parameter accepts `&Path` (not just `PathBuf`), so callers
    /// can pass borrowed paths without allocation.
    #[test]
    fn load_fixture_accepts_borrowed_path() {
        let dir = tempdir_unique();
        let path = dir.join("p.json");
        std::fs::write(&path, b"hi").expect("write");
        let result = load_fixture(path.as_path());
        assert_eq!(result.expect("read"), "hi");
    }

    /// Creates a unique temp directory under the OS temp area. We avoid
    /// pulling in the `tempfile` crate as a dependency just for tests; the
    /// standard library is enough.
    fn tempdir_unique() -> std::path::PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("openstream-testkit-{pid}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
