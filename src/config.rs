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
use std::fmt;
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

/// A validation error produced by [`M1ToolsConfig::validate`].
///
/// Each variant carries enough context for a CLI to print one actionable line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// `[lint] max_line_length` must be ≥ 1 and ≤ 1000.
    MaxLineLengthOutOfRange(usize),
    /// `[format] indent_width` must be ≥ 1 and ≤ 16.
    IndentWidthOutOfRange(usize),
    /// `[lint] max_nesting_depth` must be ≥ 1.
    MaxNestingDepthZero,
    /// `[lint] max_cognitive_complexity` must be ≥ 1.
    MaxCognitiveComplexityZero,
    /// `[lint] max_complexity` must be ≥ 1.
    MaxComplexityZero,
    /// A diagnostic code in `[diagnostics] ignore` or `select` is malformed.
    ///
    /// Valid codes are either tool codes matching `[A-Z][0-9]{3}` (`T041`,
    /// `L010`) or named kebab-case codes (`syntax-error`, `unsupported-c-token`).
    MalformedDiagnosticCode(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MaxLineLengthOutOfRange(v) => {
                write!(f, "max_line_length must be ≥ 1 and ≤ 1000; got {v}")
            }
            ConfigError::IndentWidthOutOfRange(v) => {
                write!(f, "indent_width must be ≥ 1 and ≤ 16; got {v}")
            }
            ConfigError::MaxNestingDepthZero => {
                write!(f, "max_nesting_depth must be ≥ 1; got 0")
            }
            ConfigError::MaxCognitiveComplexityZero => {
                write!(f, "max_cognitive_complexity must be ≥ 1; got 0")
            }
            ConfigError::MaxComplexityZero => {
                write!(f, "max_complexity must be ≥ 1; got 0")
            }
            ConfigError::MalformedDiagnosticCode(code) => write!(
                f,
                "malformed diagnostic code {code:?}; expected an uppercase letter + 3 digits (e.g. T041, L010) or a named code (e.g. syntax-error)"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Returns `true` if `code` is a valid diagnostic code. Two shapes exist:
/// tool codes — one uppercase ASCII letter followed by exactly three ASCII
/// digits (`T041`, `L010`) — and the named, digit-free kebab-case codes the
/// LSP layer publishes for m1-core diagnostics (`syntax-error`,
/// `unsupported-c-token`). Anything else (notably lowercased or truncated
/// tool codes like `t041`/`T41`) is a config mistake worth flagging.
fn is_valid_diagnostic_code(code: &str) -> bool {
    let b = code.as_bytes();
    let tool_code = b.len() == 4
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit();
    let named_code = !b.is_empty()
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.last().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|c| c.is_ascii_lowercase() || *c == b'-');
    tool_code || named_code
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

    /// Validate the configuration values, returning all errors collected.
    ///
    /// Returns `Ok(())` if everything is in range, or `Err(errors)` with at
    /// least one [`ConfigError`] describing each problem. Every error carries
    /// enough context for a CLI to print one actionable line per error.
    ///
    /// Note: `indent_style` and `brace_style` are stored as raw `String`s and
    /// are NOT validated here — each tool resolves them via
    /// [`crate::IndentStyle::parse`] / [`crate::BraceStyle::parse`] at the
    /// point of use, where it can apply a sensible fallback.
    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        // [lint] max_line_length: must be 1..=1000 when set.
        if let Some(v) = self.lint.max_line_length
            && (v == 0 || v > 1000)
        {
            errors.push(ConfigError::MaxLineLengthOutOfRange(v));
        }

        // [lint] max_nesting_depth: must be ≥ 1 when set.
        if let Some(v) = self.lint.max_nesting_depth
            && v == 0
        {
            errors.push(ConfigError::MaxNestingDepthZero);
        }

        // [lint] max_cognitive_complexity: must be ≥ 1 when set.
        if let Some(v) = self.lint.max_cognitive_complexity
            && v == 0
        {
            errors.push(ConfigError::MaxCognitiveComplexityZero);
        }

        // [lint] max_complexity (cyclomatic, L009): must be ≥ 1 when set.
        if let Some(v) = self.lint.max_complexity
            && v == 0
        {
            errors.push(ConfigError::MaxComplexityZero);
        }

        // [format] indent_width: must be 1..=16 when set.
        if let Some(v) = self.format.indent_width
            && (v == 0 || v > 16)
        {
            errors.push(ConfigError::IndentWidthOutOfRange(v));
        }

        // [diagnostics] ignore / select: each code must match [A-Z][0-9]{3}.
        let all_codes = self
            .diagnostics
            .ignore
            .iter()
            .flatten()
            .chain(self.diagnostics.select.iter().flatten());
        for code in all_codes {
            if !is_valid_diagnostic_code(code) {
                errors.push(ConfigError::MalformedDiagnosticCode(code.clone()));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
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

    // ── Issue #17: M1ToolsConfig::validate() ────────────────────────────────

    #[test]
    fn validate_default_config_is_ok() {
        let c = M1ToolsConfig::default();
        assert!(c.validate().is_ok(), "default config must be valid");
    }

    #[test]
    fn validate_accepts_reasonable_values() {
        let c = M1ToolsConfig::from_toml_str(
            "[lint]\nmax_line_length = 120\nmax_nesting_depth = 8\nmax_cognitive_complexity = 15\n\
             [format]\nindent_width = 4\n\
             [diagnostics]\nignore = [\"T041\", \"L010\"]\nselect = [\"T002\"]\n",
        )
        .unwrap();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_max_line_length() {
        let c = M1ToolsConfig::from_toml_str("[lint]\nmax_line_length = 0\n").unwrap();
        let errs = c.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::MaxLineLengthOutOfRange(0))),
            "expected MaxLineLengthOutOfRange(0), got {errs:?}"
        );
    }

    #[test]
    fn validate_rejects_absurd_max_line_length() {
        let c = M1ToolsConfig::from_toml_str("[lint]\nmax_line_length = 1001\n").unwrap();
        let errs = c.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::MaxLineLengthOutOfRange(1001))),
            "expected MaxLineLengthOutOfRange(1001), got {errs:?}"
        );
    }

    #[test]
    fn validate_accepts_boundary_max_line_length() {
        // 1 and 1000 are both valid (inclusive).
        let lo = M1ToolsConfig::from_toml_str("[lint]\nmax_line_length = 1\n").unwrap();
        assert!(lo.validate().is_ok());
        let hi = M1ToolsConfig::from_toml_str("[lint]\nmax_line_length = 1000\n").unwrap();
        assert!(hi.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_indent_width() {
        let c = M1ToolsConfig::from_toml_str("[format]\nindent_width = 0\n").unwrap();
        let errs = c.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::IndentWidthOutOfRange(0))),
            "expected IndentWidthOutOfRange(0), got {errs:?}"
        );
    }

    #[test]
    fn validate_rejects_large_indent_width() {
        let c = M1ToolsConfig::from_toml_str("[format]\nindent_width = 17\n").unwrap();
        let errs = c.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::IndentWidthOutOfRange(17))),
            "expected IndentWidthOutOfRange(17), got {errs:?}"
        );
    }

    #[test]
    fn validate_accepts_boundary_indent_width() {
        let lo = M1ToolsConfig::from_toml_str("[format]\nindent_width = 1\n").unwrap();
        assert!(lo.validate().is_ok());
        let hi = M1ToolsConfig::from_toml_str("[format]\nindent_width = 16\n").unwrap();
        assert!(hi.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_max_nesting_depth() {
        let c = M1ToolsConfig::from_toml_str("[lint]\nmax_nesting_depth = 0\n").unwrap();
        let errs = c.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::MaxNestingDepthZero)),
            "expected MaxNestingDepthZero, got {errs:?}"
        );
    }

    #[test]
    fn validate_rejects_zero_max_cognitive_complexity() {
        let c = M1ToolsConfig::from_toml_str("[lint]\nmax_cognitive_complexity = 0\n").unwrap();
        let errs = c.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::MaxCognitiveComplexityZero)),
            "expected MaxCognitiveComplexityZero, got {errs:?}"
        );
    }

    #[test]
    fn validate_rejects_malformed_diagnostic_codes() {
        let c = M1ToolsConfig::from_toml_str(
            "[diagnostics]\nignore = [\"T041\", \"t001\", \"T41\", \"Syntax-Error\", \"L010\"]\n",
        )
        .unwrap();
        let errs = c.validate().unwrap_err();
        // Lowercased/truncated tool codes and mixed-case names are config
        // mistakes; "T041" and "L010" are valid.
        let bad: Vec<_> = errs
            .iter()
            .filter_map(|e| {
                if let ConfigError::MalformedDiagnosticCode(s) = e {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            bad.contains(&"t001"),
            "expected t001 (lowercase tool code) invalid, got {errs:?}"
        );
        assert!(
            bad.contains(&"T41"),
            "expected T41 (truncated) invalid, got {errs:?}"
        );
        assert!(
            bad.contains(&"Syntax-Error"),
            "expected Syntax-Error (mixed case) invalid, got {errs:?}"
        );
        assert!(!bad.contains(&"T041"), "T041 must be valid");
        assert!(!bad.contains(&"L010"), "L010 must be valid");
    }

    #[test]
    fn validate_accepts_named_core_codes() {
        // The LSP layer filters on named kebab-case codes for m1-core
        // diagnostics — these are valid in ignore/select.
        let c = M1ToolsConfig::from_toml_str(
            "[diagnostics]\nignore = [\"unsupported-c-token\", \"syntax-error\"]\n\
             select = [\"missing-token\", \"annotation\", \"T080\"]\n",
        )
        .unwrap();
        assert!(
            c.validate().is_ok(),
            "named core codes must be valid: {:?}",
            c.validate()
        );
    }

    #[test]
    fn validate_rejects_malformed_codes_in_select() {
        let c =
            M1ToolsConfig::from_toml_str("[diagnostics]\nselect = [\"B4D\", \"E001\"]\n").unwrap();
        let errs = c.validate().unwrap_err();
        let bad: Vec<_> = errs
            .iter()
            .filter_map(|e| {
                if let ConfigError::MalformedDiagnosticCode(s) = e {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(bad.contains(&"B4D"), "expected 'B4D' invalid");
        assert!(!bad.contains(&"E001"), "E001 must be valid");
    }

    #[test]
    fn validate_rejects_zero_max_complexity() {
        let c = M1ToolsConfig::from_toml_str("[lint]\nmax_complexity = 0\n").unwrap();
        let errs = c.validate().unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::MaxComplexityZero)),
            "expected MaxComplexityZero, got {errs:?}"
        );
    }

    #[test]
    fn validate_collects_multiple_errors() {
        // Both max_line_length = 0 and indent_width = 0 should be reported.
        let c = M1ToolsConfig::from_toml_str(
            "[lint]\nmax_line_length = 0\nmax_nesting_depth = 0\n\
             [format]\nindent_width = 0\n",
        )
        .unwrap();
        let errs = c.validate().unwrap_err();
        assert!(errs.len() >= 3, "expected ≥3 errors, got {errs:?}");
    }

    #[test]
    fn config_error_display_is_actionable() {
        // Each error variant must produce a message mentioning the value.
        let e = ConfigError::MaxLineLengthOutOfRange(0);
        let msg = e.to_string();
        assert!(
            msg.contains('0') && msg.contains("max_line_length"),
            "display must mention field and value: {msg:?}"
        );
        let e2 = ConfigError::IndentWidthOutOfRange(99);
        let msg2 = e2.to_string();
        assert!(
            msg2.contains("99") && msg2.contains("indent_width"),
            "{msg2:?}"
        );
    }
}
