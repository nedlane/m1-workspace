//! Code-style enums shared by `m1-fmt` and `m1-lint`.
//!
//! Both tools defined their own `IndentStyle` (and `m1-fmt` a `BraceStyle`) with
//! the same variants, defaults, and string spellings. This is the canonical
//! copy. The M1 Development Manual mandates tab indentation and Allman braces, so
//! those are the defaults; teams diverge via config.
//!
//! Parsing matches the originals exactly: the spellings are lowercase and matched
//! verbatim (no case folding), and an unknown spelling yields `None` from
//! [`IndentStyle::parse`] / [`BraceStyle::parse`] or an [`InvalidStyle`] error
//! from the [`std::str::FromStr`] impls.

use std::fmt;
use std::str::FromStr;

/// Indentation character. The M1 manual mandates tabs, so that is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
    #[default]
    Tab,
    Spaces,
}

impl IndentStyle {
    /// Parse a `indent_style` spelling: `"tab"`/`"tabs"` or `"space"`/`"spaces"`.
    /// Returns `None` for any other spelling.
    pub fn parse(s: &str) -> Option<IndentStyle> {
        match s {
            "tab" | "tabs" => Some(IndentStyle::Tab),
            "space" | "spaces" => Some(IndentStyle::Spaces),
            _ => None,
        }
    }
}

impl FromStr for IndentStyle {
    type Err = InvalidStyle;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        IndentStyle::parse(s).ok_or_else(|| InvalidStyle {
            kind: "indent_style",
            value: s.to_string(),
        })
    }
}

/// Brace placement. The M1 manual mandates Allman ("a separate line for each
/// brace"), so that is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BraceStyle {
    #[default]
    Allman,
    /// K&R / "one true brace": opening brace on the keyword line.
    Kr,
}

impl BraceStyle {
    /// Parse a `brace_style` spelling: `"allman"` or `"kr"`/`"k&r"`/`"knr"`.
    /// Returns `None` for any other spelling.
    pub fn parse(s: &str) -> Option<BraceStyle> {
        match s {
            "allman" => Some(BraceStyle::Allman),
            "kr" | "k&r" | "knr" => Some(BraceStyle::Kr),
            _ => None,
        }
    }
}

impl FromStr for BraceStyle {
    type Err = InvalidStyle;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BraceStyle::parse(s).ok_or_else(|| InvalidStyle {
            kind: "brace_style",
            value: s.to_string(),
        })
    }
}

/// Error returned by the [`FromStr`] impls when a style spelling is unrecognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidStyle {
    /// The setting being parsed (`"indent_style"` or `"brace_style"`).
    pub kind: &'static str,
    /// The unrecognized spelling that was supplied.
    pub value: String,
}

impl fmt::Display for InvalidStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {} {:?}", self.kind, self.value)
    }
}

impl std::error::Error for InvalidStyle {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_default_is_tab() {
        assert_eq!(IndentStyle::default(), IndentStyle::Tab);
    }

    #[test]
    fn indent_parses_all_spellings() {
        assert_eq!(IndentStyle::parse("tab"), Some(IndentStyle::Tab));
        assert_eq!(IndentStyle::parse("tabs"), Some(IndentStyle::Tab));
        assert_eq!(IndentStyle::parse("space"), Some(IndentStyle::Spaces));
        assert_eq!(IndentStyle::parse("spaces"), Some(IndentStyle::Spaces));
    }

    #[test]
    fn indent_unknown_is_none() {
        assert_eq!(IndentStyle::parse("tabz"), None);
        assert_eq!(IndentStyle::parse("TAB"), None);
    }

    #[test]
    fn indent_from_str() {
        assert_eq!("tabs".parse::<IndentStyle>(), Ok(IndentStyle::Tab));
        let err = "nope".parse::<IndentStyle>().unwrap_err();
        assert_eq!(err.kind, "indent_style");
        assert_eq!(err.value, "nope");
        assert_eq!(err.to_string(), "invalid indent_style \"nope\"");
    }

    #[test]
    fn brace_default_is_allman() {
        assert_eq!(BraceStyle::default(), BraceStyle::Allman);
    }

    #[test]
    fn brace_parses_all_spellings() {
        assert_eq!(BraceStyle::parse("allman"), Some(BraceStyle::Allman));
        assert_eq!(BraceStyle::parse("kr"), Some(BraceStyle::Kr));
        assert_eq!(BraceStyle::parse("k&r"), Some(BraceStyle::Kr));
        assert_eq!(BraceStyle::parse("knr"), Some(BraceStyle::Kr));
    }

    #[test]
    fn brace_unknown_is_none() {
        assert_eq!(BraceStyle::parse("egyptian"), None);
        assert_eq!(BraceStyle::parse("Allman"), None);
    }

    #[test]
    fn brace_from_str() {
        assert_eq!("k&r".parse::<BraceStyle>(), Ok(BraceStyle::Kr));
        let err = "nope".parse::<BraceStyle>().unwrap_err();
        assert_eq!(err.kind, "brace_style");
    }
}
