# AGENTS.md — m1-workspace

Guidance for coding agents working in this repository.

## Purpose

The shared base crate of the M1 toolchain. Every tool that loads a MoTeC M1
project (`m1-fmt`, `m1-lint`, `m1-typecheck`, `m1-lsp`, `m1-project`) needs to
find the same files, decode them the same way, and read the same
configuration. This crate exists so those rules live in exactly one place —
before it, the discovery and decoding logic was duplicated across tools and
drifted.

It covers four areas: project/file discovery and path conventions, tolerant
text decoding, the unified `m1-tools.toml` config schema, and small shared
output/style primitives (atomic writes, diffs, line indexing, indent/brace
style enums).

## Things that are deliberate (don't "fix" them)

- **Leaf crate.** It must not depend on any other m1-* crate — everything else
  depends on it. Keep third-party dependencies minimal too.
- **Tolerant decoding is the point.** Real MoTeC files are Windows-1252 in
  practice, not UTF-8; a strict `fs::read_to_string` on a real project fails.
  Don't replace the decode helpers with strict reads.
- **Config knobs are `Option<T>` defaulting to `None`.** Defaults belong to
  the consuming tools, and additive optional fields keep schema changes to a
  minor version bump.
- **Manual-conformant defaults.** Where the M1 Development Manual mandates a
  style (tabs, Allman braces), the toolchain defaults to the manual and makes
  deviation a config choice — never the other way around.

## How it's consumed and released

Consumed via **versioned git tags** (`tag = "vX.Y.Z"`), never `branch`/`path`/
`[patch]` deps — the local multi-repo checkout must build exactly like a
public clone. A release is a version bump on `main`; `release.yml` creates the
tag, and the tag is the deliverable. After releasing, open the consumer bump
PRs immediately rather than waiting for Dependabot. Schema additions here
usually cascade: consumers wire new config fields through their own resolve
layers in follow-up PRs.

## Build / test gate

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI also runs rustdoc with `-D warnings`, a security audit, and an MSRV job.
The MSRV pin in CI (`dtolnay/rust-toolchain@<version>`) must stay in sync with
`rust-version` in `Cargo.toml` — never bump one without the other.
