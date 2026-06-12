# AGENTS.md — m1-workspace

Guidance for coding agents working in this repository.

## What this is

The shared base crate of the M1 toolchain: project/file discovery, path
conventions, tolerant Windows-1252/UTF-8 text decoding, the unified
`m1-tools.toml` config schema, atomic writes, unified diff, `LineIndex`, and
the shared `IndentStyle`/`BraceStyle` enums. It is a **leaf crate**: it must
not depend on any other m1-* crate, and (almost) everything in the toolchain
depends on it.

## Module map

- `src/lib.rs` — discovery (`find_project_file`, `find_config_file`,
  `find_dbc_files`, `find_scripts`, `find_upward`) and the `Root.` prefix
  helpers (`qualify_root`/`strip_root`) plus file-name constants.
- `src/decode.rs` — tolerant text reading. MoTeC files are Windows-1252 in
  practice; a strict UTF-8 read of a real project will fail. Never replace
  these readers with `fs::read_to_string`.
- `src/config.rs` — the `m1-tools.toml` schema (`M1ToolsConfig` with
  `[format]`/`[lint]`/`[diagnostics]`), `validate()` range checks,
  `parse_ignore_symbol`. New knobs are `Option<T>` defaulting to `None`
  (additive ⇒ minor version bump) and get a `validate()` range check + tests
  when numeric.
- `src/io.rs` — `atomic_write` (temp file + rename in the target directory).
- `src/diff.rs` — `unified_diff`, shared by `m1-fmt --diff` and m1-lsp.
- `src/line_index.rs` — byte offset ↔ line/column.
- `src/style.rs` — `IndentStyle`, `BraceStyle` (manual-conformant defaults are
  tabs + Allman; the enums themselves are neutral).

## Build / test gate

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI also runs a Docs job (rustdoc with `-D warnings`), a Security Audit, and an
MSRV job pinned to Rust 1.88. The MSRV pin in `.github/workflows/ci.yml`
(`dtolnay/rust-toolchain@1.88`) must stay in sync with `rust-version` in
`Cargo.toml` — never bump one without the other (Dependabot is configured to
ignore that action for this reason).

## Releases and consumers

- Consumed via **versioned git tags** (`tag = "vX.Y.Z"`), never `branch` or
  `path`/`[patch]` deps — the local multi-repo checkout must build exactly like
  a public clone.
- Release = bump `version` in `Cargo.toml` (and `Cargo.lock`) on `main`;
  `release.yml` creates the tag. The tag is the deliverable.
- After a release, open the consumer bump PRs (m1-fmt, m1-lint, m1-typecheck,
  m1-lsp, m1-project) immediately — Dependabot's daily run is the backstop,
  not the propagation path.
- Schema changes here usually cascade: consumer crates wire new config fields
  through their own resolve layers in follow-up PRs.

## Conventions

- Commit messages: conventional (`feat:`, `fix:`, `chore:`, `docs:`).
- No AI attribution or `Co-Authored-By` trailers in commits or PRs.
- Keep this crate dependency-light (currently serde + toml only); think hard
  before adding anything.
