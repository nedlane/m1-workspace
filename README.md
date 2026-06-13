# m1-workspace

Shared workspace conventions for the
[MoTeC M1](https://www.motec.com.au/) toolchain
([C-Nucifora/m1-tools](https://github.com/C-Nucifora/m1-tools)).

Every M1 tool that loads a project — `m1-fmt`, `m1-lint`, `m1-typecheck`,
`m1-lsp` — needs to find the same files, decode them the same way, and read the
same configuration. This crate is the single source of truth for those rules,
so they live in one place instead of being copied (and drifting) across each
tool.

## What it provides

- **Project discovery and path conventions** — locate the nearest
  `Project.m1prj` / `.m1cfg` from any starting directory, enumerate a
  project's scripts and `.m1dbc` files, and apply the implicit `Root.` group
  prefix the M1 symbol model uses.
- **Tolerant text decoding** — MoTeC files are not reliably UTF-8; real
  projects carry Windows-1252 bytes (a `°` in a comment is the single byte
  `0xB0`). The decode helpers read them without data loss where a strict UTF-8
  read would fail, and can re-encode for byte-faithful round-trips.
- **Unified configuration** — the schema for `m1-tools.toml`, the
  workspace-level config file shared by the CLIs, `m1-lsp`, VS Code, and
  Neovim. Tool-specific files (`.m1fmt.toml`, `.m1lint.toml`) and CLI flags
  override it; see the
  [m1-tools configuration docs](https://github.com/C-Nucifora/m1-tools#configuration).
- **Output and style primitives** — atomic file writes, unified diffs,
  byte-offset ↔ line/column indexing, and the shared indent/brace style enums.

## Usage

A dependency-light leaf crate consumed via a versioned git tag (the same
scheme the rest of the toolchain uses). Pin the
[latest release](https://github.com/nedlane/m1-workspace/releases):

```toml
[dependencies]
m1-workspace = { git = "https://github.com/nedlane/m1-workspace.git", tag = "v0.10.0" }
```

```rust
use std::path::Path;

if let Some(prj) = m1_workspace::find_project_file(Path::new("UQR-EV/01.00")) {
    let text = m1_workspace::read_motec_xml(&prj)?;
    // parse the project …
}
```

API documentation is in the rustdoc (`cargo doc --open`).

## Development

The CI gate is `cargo test`, `cargo clippy --all-targets -- -D warnings`, and
`cargo fmt --all -- --check`, on stable and on the MSRV. Releases are cut by
bumping `version` in `Cargo.toml` on `main`; the tag is the deliverable
(source-only — consumers build from the tag).

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).

## Trademark

Independent, community-built open-source tooling for the MoTeC® M1 script
language. Not affiliated with, authorised, or endorsed by MoTeC Pty Ltd.
"MoTeC" and "M1" are trademarks of MoTeC Pty Ltd.
