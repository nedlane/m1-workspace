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
use std::path::{Path, PathBuf};

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
    /// Extra indent levels for wrapped continuation lines (m1-fmt
    /// `continuation_indent`). Range-checked like `indent_width`.
    pub continuation_indent: Option<usize>,
    /// Align consecutive `=` assignments into a column (m1-fmt #96).
    pub align_assignments: Option<bool>,
    /// Re-wrap block and line comments to the line width (m1-fmt #95).
    pub reflow_comments: Option<bool>,
}

/// `[diagnostics]` — cross-tool code filter (L-codes and T-codes).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct DiagnosticsSection {
    pub ignore: Option<Vec<String>>,
    pub select: Option<Vec<String>>,
    /// Symbol-scoped suppressions for project-level diagnostics that carry no
    /// source range (T010/T041/T050/T071/T087 …): entries of the form
    /// `"T050:Root.Engine.Speed"` suppress that code for that one symbol only.
    pub ignore_symbols: Option<Vec<String>>,
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
    /// `[format] continuation_indent` must be ≥ 1 and ≤ 16.
    ContinuationIndentOutOfRange(usize),
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
    /// An entry in `[diagnostics] ignore_symbols` is not `CODE:Symbol.Path`.
    MalformedIgnoreSymbol(String),
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
            ConfigError::ContinuationIndentOutOfRange(v) => {
                write!(f, "continuation_indent must be ≥ 1 and ≤ 16; got {v}")
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
            ConfigError::MalformedIgnoreSymbol(entry) => write!(
                f,
                "malformed ignore_symbols entry {entry:?}; expected CODE:Symbol.Path (e.g. \"T050:Root.Engine.Speed\")"
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

/// Split a `[diagnostics] ignore_symbols` entry into `(code, symbol_path)`.
/// Valid shape: a tool code (`[A-Z][0-9]{3}`), a colon, and a non-empty
/// symbol path. Returns `None` for anything else.
pub fn parse_ignore_symbol(entry: &str) -> Option<(&str, &str)> {
    let (code, path) = entry.split_once(':')?;
    let b = code.as_bytes();
    let code_ok = b.len() == 4
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit();
    if code_ok && !path.trim().is_empty() {
        Some((code, path.trim()))
    } else {
        None
    }
}

/// Why [`M1ToolsConfig::discover_result`] could not load an existing
/// `m1-tools.toml`. "No file found" is not an error — that is `Ok(None)`.
#[derive(Debug)]
pub enum DiscoverError {
    /// The file exists but could not be read.
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file was read but is not valid TOML / does not match the schema.
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscoverError::Read { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            DiscoverError::Parse { path, source } => {
                write!(f, "cannot parse {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for DiscoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DiscoverError::Read { source, .. } => Some(source),
            DiscoverError::Parse { source, .. } => Some(source),
        }
    }
}

/// A successfully discovered and parsed `m1-tools.toml`, with everything a
/// tool needs to surface config diagnostics to the user.
#[derive(Debug)]
pub struct DiscoveredConfig {
    /// Where the file was found.
    pub path: PathBuf,
    pub config: M1ToolsConfig,
    /// Dotted paths of keys present in the file but unknown to the schema
    /// (e.g. `format.line_wdith` for a typo) — empty when the file is clean.
    /// Worth warning about: an unknown key silently keeps the tool default.
    pub unknown_keys: Vec<String>,
}

impl M1ToolsConfig {
    /// Parse a `m1-tools.toml` body. Unknown keys are ignored (serde `default`);
    /// use [`Self::from_toml_str_with_unknown_keys`] to report them.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Parse a `m1-tools.toml` body and also collect the dotted paths of any
    /// keys the schema does not know (`format.line_wdith`, `lnit`, …), so a
    /// CLI/LSP can warn that a typo is silently falling back to the default.
    /// The parse itself stays as permissive as [`Self::from_toml_str`].
    pub fn from_toml_str_with_unknown_keys(
        s: &str,
    ) -> Result<(Self, Vec<String>), toml::de::Error> {
        let de = toml::de::Deserializer::parse(s)?;
        let mut unknown = Vec::new();
        let config = serde_ignored::deserialize(de, |path| unknown.push(path.to_string()))?;
        Ok((config, unknown))
    }

    /// Walk up from `start` (inclusive) for `m1-tools.toml` and return the parsed
    /// config of the first one found. `None` if none is found OR the file fails to
    /// parse — the lenient path for callers that must never abort on a malformed
    /// unified file. Consumers that can surface diagnostics should prefer
    /// [`Self::discover_result`], which distinguishes "no config" from "broken
    /// config" and reports unknown keys.
    pub fn discover(start: &Path) -> Option<Self> {
        Self::discover_result(start)
            .ok()
            .flatten()
            .map(|found| found.config)
    }

    /// Strict variant of [`Self::discover`]: walk up from `start` (inclusive)
    /// for `m1-tools.toml`. Returns `Ok(None)` when no file exists, and a
    /// [`DiscoverError`] when a file exists but cannot be read or parsed — so
    /// a malformed config surfaces as an error instead of being silently
    /// ignored. On success the [`DiscoveredConfig`] also carries any unknown
    /// keys for typo warnings.
    pub fn discover_result(start: &Path) -> Result<Option<DiscoveredConfig>, DiscoverError> {
        let Some(path) = crate::find_upward(start, TOOLS_CONFIG_FILE) else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(&path).map_err(|source| DiscoverError::Read {
            path: path.clone(),
            source,
        })?;
        let (config, unknown_keys) =
            Self::from_toml_str_with_unknown_keys(&text).map_err(|source| {
                DiscoverError::Parse {
                    path: path.clone(),
                    source,
                }
            })?;
        Ok(Some(DiscoveredConfig {
            path,
            config,
            unknown_keys,
        }))
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

        // [format] continuation_indent: same 1..=16 range as indent_width (#25).
        if let Some(v) = self.format.continuation_indent
            && (v == 0 || v > 16)
        {
            errors.push(ConfigError::ContinuationIndentOutOfRange(v));
        }

        // [diagnostics] ignore_symbols: each entry must be CODE:Symbol.Path.
        for entry in self.diagnostics.ignore_symbols.iter().flatten() {
            if parse_ignore_symbol(entry).is_none() {
                errors.push(ConfigError::MalformedIgnoreSymbol(entry.clone()));
            }
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
    fn unknown_keys_are_reported_with_dotted_paths() {
        let (c, unknown) = M1ToolsConfig::from_toml_str_with_unknown_keys(
            "[format]\nline_wdith = 100\nindent_width = 4\n\
             [lnit]\nmax_line_length = 80\n",
        )
        .unwrap();
        // The typo'd key is ignored (default kept), but reported.
        assert_eq!(c.format.line_width, None);
        assert_eq!(c.format.indent_width, Some(4));
        assert_eq!(unknown, vec!["format.line_wdith", "lnit"]);
    }

    #[test]
    fn clean_config_reports_no_unknown_keys() {
        let toml = "[lint]\nmax_line_length = 100\nmax_nesting_depth = 4\n\
             max_complexity = 10\nmax_cognitive_complexity = 20\nexclude = []\n\
             [format]\nline_width = 100\nmax_blank_lines = 2\nindent_style = \"tab\"\n\
             indent_width = 4\nbrace_style = \"allman\"\ncontinuation_indent = 1\n\
             align_assignments = false\nreflow_comments = false\n\
             [diagnostics]\nignore = []\nselect = []\nignore_symbols = []\n";
        let (_, unknown) = M1ToolsConfig::from_toml_str_with_unknown_keys(toml).unwrap();
        assert_eq!(unknown, Vec::<String>::new());
    }

    #[test]
    fn discover_result_distinguishes_missing_from_malformed() {
        let dir = std::env::temp_dir().join(format!("m1ws-cfg-{}", std::process::id()));
        let sub = dir.join("nested");
        std::fs::create_dir_all(&sub).unwrap();

        // No file anywhere up the tree we created -> Ok(None) from `dir` only
        // if no ancestor has one; instead assert the *found* cases, which are
        // hermetic to this temp tree.
        let cfg_path = dir.join(TOOLS_CONFIG_FILE);

        // Malformed file -> Err(Parse), not a silent default.
        std::fs::write(&cfg_path, "[format\nindent_width = 4\n").unwrap();
        match M1ToolsConfig::discover_result(&sub) {
            Err(DiscoverError::Parse { path, .. }) => assert_eq!(path, cfg_path),
            other => panic!("expected Parse error, got {other:?}"),
        }
        // ...while the lenient path stays lenient.
        assert!(M1ToolsConfig::discover(&sub).is_none());

        // Valid file -> found, parsed, unknown keys reported.
        std::fs::write(&cfg_path, "[format]\nindent_width = 2\nline_wdith = 9\n").unwrap();
        let found = M1ToolsConfig::discover_result(&sub).unwrap().unwrap();
        assert_eq!(found.path, cfg_path);
        assert_eq!(found.config.format.indent_width, Some(2));
        assert_eq!(found.unknown_keys, vec!["format.line_wdith"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

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
    fn parses_format_continuation_and_comment_knobs() {
        // #25: the unified [format] section can now express the m1-fmt knobs that
        // previously needed a .m1fmt.toml side file.
        let c = M1ToolsConfig::from_toml_str(
            "[format]\ncontinuation_indent = 2\nalign_assignments = true\nreflow_comments = false\n",
        )
        .unwrap();
        assert_eq!(c.format.continuation_indent, Some(2));
        assert_eq!(c.format.align_assignments, Some(true));
        assert_eq!(c.format.reflow_comments, Some(false));
        assert!(c.validate().is_ok());
    }

    #[test]
    fn validate_rejects_out_of_range_continuation_indent() {
        for bad in [0usize, 17] {
            let c =
                M1ToolsConfig::from_toml_str(&format!("[format]\ncontinuation_indent = {bad}\n"))
                    .unwrap();
            let errs = c.validate().unwrap_err();
            assert!(
                errs.iter().any(
                    |e| matches!(e, ConfigError::ContinuationIndentOutOfRange(v) if *v == bad)
                ),
                "expected ContinuationIndentOutOfRange({bad}), got {errs:?}"
            );
        }
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

    #[test]
    fn ignore_symbols_valid_entries_pass() {
        let c = M1ToolsConfig::from_toml_str(
            "[diagnostics]\nignore_symbols = [\"T050:Root.Engine.Speed\", \"T041:Root.X.Y\"]\n",
        )
        .unwrap();
        assert!(c.validate().is_ok());
        assert_eq!(
            super::parse_ignore_symbol("T050:Root.Engine.Speed"),
            Some(("T050", "Root.Engine.Speed"))
        );
    }

    #[test]
    fn ignore_symbols_malformed_entries_flag() {
        let c = M1ToolsConfig::from_toml_str(
            "[diagnostics]\nignore_symbols = [\"T050\", \"t050:Root.X\", \"T050:\"]\n",
        )
        .unwrap();
        let errs = c.validate().unwrap_err();
        let bad: Vec<_> = errs
            .iter()
            .filter(|e| matches!(e, ConfigError::MalformedIgnoreSymbol(_)))
            .collect();
        assert_eq!(bad.len(), 3, "all three malformed: {errs:?}");
    }
}
