# m1-workspace

Shared workspace conventions for the
[MoTeC M1](https://www.motec.com.au/) toolchain
([C-Nucifora/m1-tools](https://github.com/C-Nucifora/m1-tools)).

Every M1 tool that loads a project — `m1-fmt`, `m1-lint`, `m1-typecheck`,
`m1-lsp` — needs to find the same files, decode them the same way, and read the
same configuration. This crate is the single source of truth for those rules, so
they live in one place instead of being copied (and drifting) across each tool.

## What it provides

### Project discovery and path conventions

| Item | Purpose |
|------|---------|
| `find_project_file(start)` | The nearest `Project.m1prj`, searching `start` and its ancestors. |
| `find_config_file(start)` | The nearest `*.m1cfg`, searching `start` and its ancestors (nearest wins). |
| `find_dbc_files(root)` / `find_scripts(root)` | Every `*.m1dbc` / `*.m1scr` under `root`, recursively, sorted. |
| `find_upward(start, name)` | Generic ancestor walk for an exact file name. |
| `qualify_root(name)` / `strip_root(path)` | Add/remove the implicit `Root.` group prefix — a real `.m1cfg` lists `Foo.Bar`, the symbol model keys `Root.Foo.Bar`. |
| `PROJECT_FILE`, `SCRIPT_EXT`, `CONFIG_EXT`, `DBC_EXT`, `ROOT_PREFIX` | The M1 file-name / extension / prefix constants. |

### Tolerant text decoding

MoTeC files are not reliably UTF-8 — real projects carry Windows-1252 bytes
(a `°` in a comment is the single byte `0xB0`). The `decode` module reads them
without data loss where a strict UTF-8 read would fail:

| Item | Purpose |
|------|---------|
| `read_text(path)` / `read_text_capped(path, max)` | Read a file with UTF-8 → Windows-1252 fallback. |
| `read_motec_xml(path)` | Read MoTeC XML (`.m1prj`, `.m1dbc`, …), which is Windows-1252 in practice. |
| `read_text_with_encoding(path)` / `decode_with_encoding(bytes)` | As above, also reporting which `Encoding` was detected. |
| `encode(s, encoding)` / `encode_checked(s, encoding)` | Re-encode for byte-faithful round-trips (checked variant reports lossy characters). |

### Unified configuration (`m1-tools.toml`)

The schema for the workspace-level config file shared by the CLIs, `m1-lsp`,
VS Code, and Neovim — `M1ToolsConfig` with `[format]`, `[lint]`, and
`[diagnostics]` sections, plus `validate()` range checks and
`parse_ignore_symbol` for `CODE:Symbol.Path` diagnostic filters. Tool-specific
files (`.m1fmt.toml`, `.m1lint.toml`) and CLI flags override it; see the
[m1-tools configuration docs](https://github.com/C-Nucifora/m1-tools#configuration).

### Output and style primitives

| Item | Purpose |
|------|---------|
| `atomic_write(path, bytes)` | Temp-file + rename write, so a crash never truncates the target. |
| `unified_diff(name, original, formatted)` | Minimal unified diff, shared by `m1-fmt --diff` and the LSP. |
| `LineIndex` | Byte-offset ↔ line/column conversion over a source text. |
| `IndentStyle`, `BraceStyle` | The shared `tab`/`spaces` and `allman`/`kr` style enums. |

## Usage

A dependency-free leaf crate consumed via a versioned git tag (the same scheme
the rest of the toolchain uses), so Dependabot keeps consumers current. Pin the
[latest release](https://github.com/nedlane/m1-workspace/releases):

```toml
[dependencies]
m1-workspace = { git = "https://github.com/nedlane/m1-workspace.git", tag = "v0.9.0" }
```

```rust
use std::path::Path;

if let Some(prj) = m1_workspace::find_project_file(Path::new("UQR-EV/01.00")) {
    let text = m1_workspace::read_motec_xml(&prj)?;
    // parse the project …
}
```

## Development

The CI gate is `cargo test`, `cargo clippy --all-targets -- -D warnings`, and
`cargo fmt --all -- --check`, on stable and on the MSRV (Rust 1.88). Releases
are cut by bumping `version` in `Cargo.toml` on `main`; the tag is the
deliverable (source-only — consumers build from the tag).

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).

## Trademark

Independent, community-built open-source tooling for the MoTeC® M1 script
language. Not affiliated with, authorised, or endorsed by MoTeC Pty Ltd.
"MoTeC" and "M1" are trademarks of MoTeC Pty Ltd.
