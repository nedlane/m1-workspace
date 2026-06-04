//! Shared filesystem and path conventions for the MoTeC M1 toolchain.
//!
//! Every M1 tool that loads a project (`m1-typecheck`, `m1-lsp`, …) needs to
//! locate the same files and reconcile the same path conventions: the project's
//! `Project.m1prj`, its `parameters.m1cfg`, any `*.m1dbc` CAN databases, and the
//! implicit `Root.` group prefix that fully-qualified symbol paths carry but that
//! a `.m1cfg` export omits. Each tool used to carry its own (subtly divergent)
//! copy of these rules; this crate is the single source of truth so a fix lands
//! in one place.

mod decode;
pub use decode::{
    Encoding, decode, decode_with_encoding, encode, read_motec_xml, read_text,
    read_text_with_encoding,
};

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The project file name.
pub const PROJECT_FILE: &str = "Project.m1prj";
/// The M1 script file extension (without the dot).
pub const SCRIPT_EXT: &str = "m1scr";
/// The parameter-config file extension (without the dot).
pub const CONFIG_EXT: &str = "m1cfg";
/// The CAN-database file extension (without the dot).
pub const DBC_EXT: &str = "m1dbc";
/// The implicit root group prefix on fully-qualified symbol paths (`Root.Foo.Bar`).
pub const ROOT_PREFIX: &str = "Root.";

/// Walk up from `start` (inclusive) through ancestor directories, returning the
/// first directory that contains a file named `file_name`.
pub fn find_upward(start: &Path, file_name: &str) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let cand = d.join(file_name);
        if cand.is_file() {
            return Some(cand);
        }
        dir = d.parent();
    }
    None
}

/// Locate `Project.m1prj` at `start` or any ancestor directory.
pub fn find_project_file(start: &Path) -> Option<PathBuf> {
    find_upward(start, PROJECT_FILE)
}

/// Locate the nearest `*.m1cfg`, searching `start` and its ancestor directories
/// (nearest wins). Real projects keep `parameters.m1cfg` at the repository root
/// while the `.m1prj` sits in a nested subdirectory, so a single-directory search
/// is not enough.
pub fn find_config_file(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if let Some(found) = first_with_ext(d, CONFIG_EXT) {
            return Some(found);
        }
        dir = d.parent();
    }
    None
}

/// All `*.m1dbc` files under `root`, searched recursively (CAN databases
/// typically live in a `dbc/` subdirectory). Sorted for deterministic order.
pub fn find_dbc_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_ext(root, DBC_EXT, &mut out);
    out.sort();
    out
}

/// Prepend the implicit `Root.` group prefix unless `name` is already qualified
/// (a real `.m1cfg` lists parameters by their unprefixed name, e.g. `Foo.Bar`,
/// whereas the symbol model keys them as `Root.Foo.Bar`).
pub fn qualify_root(name: &str) -> Cow<'_, str> {
    if name == "Root" || name.starts_with(ROOT_PREFIX) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("{ROOT_PREFIX}{name}"))
    }
}

/// Strip a leading `Root.` from a fully-qualified path, if present.
pub fn strip_root(path: &str) -> &str {
    path.strip_prefix(ROOT_PREFIX).unwrap_or(path)
}

/// `std::fs::read_dir`, but treat an *empty* path as the current directory.
///
/// A bare relative project filename (`Project.m1prj`) has
/// `Path::parent() == Some("")`, and `read_dir("")` errors with ENOENT where
/// `read_dir(".")` succeeds. The cfg/dbc ancestor walk fed that empty parent
/// straight to `read_dir`, so a project invoked as `--project Project.m1prj`
/// (vs `./Project.m1prj`) silently found no `.m1cfg`/`.m1dbc` — making
/// m1-typecheck's T041/T042 no-op (see m1-typecheck#87).
fn read_dir_normalized(dir: &Path) -> std::io::Result<std::fs::ReadDir> {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    std::fs::read_dir(dir)
}

fn first_with_ext(dir: &Path, ext: &str) -> Option<PathBuf> {
    read_dir_normalized(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        (p.extension().and_then(|x| x.to_str()) == Some(ext)).then_some(p)
    })
}

fn walk_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = read_dir_normalized(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_ext(&p, ext, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_project_in_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join(PROJECT_FILE), "<x/>").unwrap();
        let nested = root.join("UQR-EV").join("01.00");
        fs::create_dir_all(&nested).unwrap();
        let found = find_project_file(&nested).unwrap();
        assert_eq!(found, root.join(PROJECT_FILE));
    }

    #[test]
    fn no_project_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(find_project_file(tmp.path()).is_none());
    }

    #[test]
    fn finds_cfg_in_ancestor_nearest_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("root.m1cfg"), "").unwrap();
        let mid = root.join("a");
        let deep = mid.join("b");
        fs::create_dir_all(&deep).unwrap();
        fs::write(mid.join("near.m1cfg"), "").unwrap();
        // From `deep`, the nearest cfg is the one in `mid`, not the root.
        assert_eq!(find_config_file(&deep).unwrap(), mid.join("near.m1cfg"));
    }

    #[test]
    fn finds_dbc_recursively_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("dbc");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("b.m1dbc"), "").unwrap();
        fs::write(sub.join("a.m1dbc"), "").unwrap();
        fs::write(root.join("c.txt"), "").unwrap();
        let found = find_dbc_files(root);
        assert_eq!(found, vec![sub.join("a.m1dbc"), sub.join("b.m1dbc")]);
    }

    #[test]
    fn read_dir_normalized_treats_empty_path_as_cwd() {
        // `read_dir("")` errors (ENOENT); the helper reads "." instead, so the
        // cfg/dbc ancestor walk reaches the cwd for a bare relative project name.
        assert!(std::fs::read_dir(Path::new("")).is_err());
        assert!(read_dir_normalized(Path::new("")).is_ok());
    }

    #[test]
    fn qualify_and_strip_root() {
        assert_eq!(qualify_root("Foo.Bar"), "Root.Foo.Bar");
        assert_eq!(qualify_root("Root.Foo.Bar"), "Root.Foo.Bar");
        assert_eq!(qualify_root("Root"), "Root");
        assert_eq!(strip_root("Root.Foo.Bar"), "Foo.Bar");
        assert_eq!(strip_root("Foo.Bar"), "Foo.Bar");
    }
}
