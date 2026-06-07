//! Atomic file writes shared by the formatting/linting CLIs.
//!
//! `m1-fmt` and `m1-lint` each carried a byte-identical `atomic_write`; this is
//! the single canonical copy so the in-place rewrite behaviour (no partial
//! writes, no cross-filesystem rename) lives in one place.

use std::path::Path;

/// Write `contents` to `path` atomically: a same-directory temp file is written,
/// flushed, and `fsync`ed, then renamed over `path`. On any failure the temp
/// file is removed so a partial write never lingers.
///
/// The temp file is created in `path`'s parent so the final rename stays on one
/// filesystem (a cross-filesystem rename is not atomic and would fail). The pid
/// plus the target file name keep concurrent runs from colliding on the temp
/// path.
pub fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().map(|s| s.to_owned()).unwrap_or_default();
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(&file_name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = dir.join(tmp_name);

    // Scope the file handle so it is closed before the rename; clean up the temp
    // on any failure so a partial write never lingers.
    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.flush()?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn writes_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.txt");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn overwrites_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.txt");
        fs::write(&path, b"old contents that are longer").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn writes_exact_bytes_including_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.bin");
        let bytes: Vec<u8> = (0u8..=255).collect();
        atomic_write(&path, &bytes).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn leaves_no_temp_file_on_success() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.txt");
        atomic_write(&path, b"x").unwrap();
        let entries: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("out.txt")]);
    }
}
