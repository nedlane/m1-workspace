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
///
/// When `path` already exists, its Unix permission mode is preserved: the temp
/// file (created with the umask default) is `chmod`ed to match before the rename,
/// so a tightened mode like `0o600` is not silently widened on every rewrite. A
/// new file keeps the usual umask default.
pub fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().map(|s| s.to_owned()).unwrap_or_default();
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(&file_name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = dir.join(tmp_name);

    // Mode of the file being replaced, so the rewrite preserves it rather than
    // dropping back to the umask default. `None` for a new file (or a non-Unix
    // platform), where the umask default is the right behaviour.
    let existing_mode = existing_mode(path);

    // Scope the file handle so it is closed before the rename; clean up the temp
    // on any failure so a partial write never lingers.
    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.flush()?;
        f.sync_all()?;
        apply_mode(&f, existing_mode)?;
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

#[cfg(unix)]
fn existing_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    // `symlink_metadata` deliberately does NOT follow a symlink: the mode we want
    // is that of the regular file the rename will replace, and a symlink's own
    // mode is not meaningful here.
    std::fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn existing_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(unix)]
fn apply_mode(f: &std::fs::File, mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        f.set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_mode(_f: &std::fs::File, _mode: Option<u32>) -> std::io::Result<()> {
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

    #[cfg(unix)]
    #[test]
    fn preserves_mode_of_existing_file() {
        // Overwriting via a fresh temp + rename must not silently widen the
        // permissions of the file being replaced. A tightened mode (e.g. 0o600)
        // would otherwise become the umask default (typically 0o644) on every
        // format/lint write.
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.txt");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode should be preserved, got {mode:o}");
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
