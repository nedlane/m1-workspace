//! The unified `m1-tools.toml` schema and discovery — the single source of truth
//! shared by every M1 tool (the CLIs and the LSP).
//!
//! This is a *raw*, all-optional view: each field is `Option<_>` so a tool can
//! layer it under a lower-precedence default and over a higher-precedence
//! override. The crate deliberately does not depend on `m1-fmt`/`m1-lint`, so the
//! mapping from these raw values onto each tool's own typed options lives in that
//! tool (no dependency cycle). `indent_style` lives under `[format]` only — it is
//! one decision shared by the formatter and the linter; both read it from there.

use serde::Deserialize;
use std::path::Path;

/// The unified config file name, discovered by walking up from a working dir.
pub const TOOLS_CONFIG_FILE: &str = "m1-tools.toml";

/// The whole `m1-tools.toml`, every field optional. Also deserialises from the
/// editor-settings JSON the LSP receives (same snake_case shape).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct M1ToolsConfig {
    pub lint: LintSection,
    pub format: FormatSection,
    pub diagnostics: DiagnosticsSection,
}

/// `[lint]` — thresholds and file excludes. (Indent lives under `[format]`.)
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LintSection {
    pub max_line_length: Option<usize>,
    pub max_nesting_depth: Option<usize>,
    pub max_complexity: Option<u32>,
    pub max_cognitive_complexity: Option<u32>,
    pub exclude: Option<Vec<String>>,
}

/// `[format]` — formatter options, plus the shared `indent_style`.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct FormatSection {
    pub line_width: Option<usize>,
    pub max_blank_lines: Option<usize>,
    /// `"tab"` | `"spaces"`. Shared by the formatter and the linter (L010).
    pub indent_style: Option<String>,
    pub indent_width: Option<usize>,
    /// `"allman"` | `"kr"`.
    pub brace_style: Option<String>,
}

/// `[diagnostics]` — cross-tool code filter (L-codes and T-codes).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct DiagnosticsSection {
    pub ignore: Option<Vec<String>>,
    pub select: Option<Vec<String>>,
}

impl M1ToolsConfig {
    /// Parse a `m1-tools.toml` body. Unknown keys are ignored (serde `default`).
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Walk up from `start` (inclusive) for `m1-tools.toml` and return the parsed
    /// config of the first one found. `None` if none is found OR the file fails to
    /// parse — the lenient path the CLIs use so a malformed unified file never
    /// aborts a run (callers wanting hard errors use [`Self::from_toml_str`]).
    pub fn discover(start: &Path) -> Option<Self> {
        let path = crate::find_upward(start, TOOLS_CONFIG_FILE)?;
        let text = std::fs::read_to_string(&path).ok()?;
        Self::from_toml_str(&text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_sections() {
        let c = M1ToolsConfig::from_toml_str(
            "[lint]\nmax_line_length = 100\nmax_cognitive_complexity = 20\nexclude = [\"*.gen.m1scr\"]\n\
             [format]\nbrace_style = \"kr\"\nindent_style = \"spaces\"\nindent_width = 2\n\
             [diagnostics]\nignore = [\"T041\"]\n",
        )
        .unwrap();
        assert_eq!(c.lint.max_line_length, Some(100));
        assert_eq!(c.lint.max_cognitive_complexity, Some(20));
        assert_eq!(
            c.lint.exclude.as_deref(),
            Some(&["*.gen.m1scr".to_string()][..])
        );
        assert_eq!(c.format.brace_style.as_deref(), Some("kr"));
        assert_eq!(c.format.indent_style.as_deref(), Some("spaces"));
        assert_eq!(c.format.indent_width, Some(2));
        assert_eq!(
            c.diagnostics.ignore.as_deref(),
            Some(&["T041".to_string()][..])
        );
    }

    #[test]
    fn unset_fields_are_none() {
        let c = M1ToolsConfig::from_toml_str("[format]\nbrace_style = \"kr\"\n").unwrap();
        assert_eq!(c.format.brace_style.as_deref(), Some("kr"));
        assert_eq!(c.format.line_width, None);
        assert_eq!(c.lint.max_line_length, None);
    }

    #[test]
    fn unknown_keys_ignored() {
        let c =
            M1ToolsConfig::from_toml_str("[format]\nfuture = 1\nbrace_style = \"kr\"\n").unwrap();
        assert_eq!(c.format.brace_style.as_deref(), Some("kr"));
    }

    #[test]
    fn discover_walks_up_and_returns_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(M1ToolsConfig::discover(&nested).is_none());
        std::fs::write(
            tmp.path().join("m1-tools.toml"),
            "[format]\nbrace_style = \"kr\"\n",
        )
        .unwrap();
        let c = M1ToolsConfig::discover(&nested).expect("found up the tree");
        assert_eq!(c.format.brace_style.as_deref(), Some("kr"));
    }
}
