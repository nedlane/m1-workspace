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
/// The temp file is exclusively created with a unique name in `path`'s parent,
/// so the final rename stays on one filesystem and concurrent writers do not
/// share temporary files. Only the temporary file owned by this call is removed
/// on failure.
///
/// When `path` already exists, its Unix permission mode is applied before any
/// contents are written. A new file keeps the usual umask default.
pub fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mode = existing_mode(path);
    let mut builder = tempfile::Builder::new();
    builder.prefix(".m1-").suffix(".tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(mode.unwrap_or(0o666)));
    }
    let mut temp = builder.tempfile_in(dir)?;
    apply_mode(temp.as_file(), mode)?;
    temp.write_all(contents)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
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
    #[cfg(unix)]
    #[test]
    fn existing_temp_links_cannot_redirect_writes() {
        use std::os::unix::fs::symlink;
        for hard_link in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("out");
            let victim = dir.path().join("victim");
            let occupied = dir.path().join(format!(".out.{}.tmp", std::process::id()));
            fs::write(&victim, b"valuable data").unwrap();
            if hard_link {
                fs::hard_link(&victim, &occupied).unwrap();
            } else {
                symlink(&victim, &occupied).unwrap();
            }
            atomic_write(&path, b"replacement").unwrap();
            assert_eq!(fs::read(&victim).unwrap(), b"valuable data");
            assert_eq!(fs::read(&occupied).unwrap(), b"valuable data");
            assert!(path.symlink_metadata().unwrap().is_file());
            assert_eq!(fs::read(&path).unwrap(), b"replacement");
        }
    }

    #[test]
    fn preserves_occupied_temp_file_and_cleans_failed_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out");
        let occupied = dir.path().join(format!(".out.{}.tmp", std::process::id()));
        fs::write(&occupied, b"another writer").unwrap();
        fs::create_dir(&path).unwrap();
        assert!(atomic_write(&path, b"replacement").is_err());
        assert_eq!(fs::read(&occupied).unwrap(), b"another writer");
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn concurrent_writes_install_whole_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out");
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for value in 0..8u8 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    let bytes = vec![value; 65536];
                    barrier.wait();
                    for _ in 0..8 {
                        atomic_write(path, &bytes).unwrap();
                        let read = fs::read(path).unwrap();
                        assert_eq!(read.len(), bytes.len());
                        assert!(read.iter().all(|b| *b == read[0]));
                    }
                });
            }
        });
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
