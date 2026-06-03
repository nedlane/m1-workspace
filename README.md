# m1-workspace

Shared filesystem and path conventions for the
[MoTeC M1](https://www.motec.com.au/) toolchain
([C-Nucifora/m1-tools](https://github.com/C-Nucifora/m1-tools)).

Every M1 tool that loads a project — `m1-typecheck`, `m1-lsp`, and anything else
that resolves the symbol model — needs to find the same files and honour the same
path conventions. This crate is the single source of truth for those rules, so
they live in one place instead of being copied (and drifting) across each tool.

## What it provides

| Item | Purpose |
|------|---------|
| `find_project_file(start)` | The nearest `Project.m1prj`, searching `start` and its ancestors. |
| `find_config_file(start)` | The nearest `*.m1cfg`, searching `start` and its ancestors (nearest wins). |
| `find_dbc_files(root)` | Every `*.m1dbc` under `root`, recursively, sorted. |
| `find_upward(start, name)` | Generic ancestor walk for an exact file name. |
| `qualify_root(name)` / `strip_root(path)` | Add/remove the implicit `Root.` group prefix — a real `.m1cfg` lists `Foo.Bar`, the symbol model keys `Root.Foo.Bar`. |
| `PROJECT_FILE`, `SCRIPT_EXT`, `CONFIG_EXT`, `DBC_EXT`, `ROOT_PREFIX` | The M1 file-name / extension / prefix constants. |

## Usage

It is a path-dependency-free leaf crate consumed via a versioned git tag (the same
scheme the rest of the toolchain uses), so Dependabot keeps consumers current:

```toml
[dependencies]
m1-workspace = { git = "https://github.com/nedlane/m1-workspace.git", tag = "v0.1.0" }
```

```rust
use std::path::Path;

if let Some(prj) = m1_workspace::find_project_file(Path::new("UQR-EV/01.00")) {
    let cfg = prj.parent().and_then(m1_workspace::find_config_file);
    // load the project + cfg …
}
```

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).

## Trademark

Independent, community-built open-source tooling for the MoTeC® M1 script
language. Not affiliated with, authorised, or endorsed by MoTeC Pty Ltd.
"MoTeC" and "M1" are trademarks of MoTeC Pty Ltd.
