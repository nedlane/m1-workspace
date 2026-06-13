//! Byte-offset → line/column lookup table.
//!
//! Canonicalizes the `LineIndex` that `m1-typecheck` built for its XML parser:
//! a one-time scan records every line start, after which a byte offset maps to
//! its 0-based source line in O(log N) via binary search — replacing a rescan
//! from byte 0 on every call (which made per-component lookups O(N^2)).
//!
//! Dependency-light by design (no `m1-core`): offsets and columns are in **bytes**,
//! not Unicode scalar values or UTF-16 units, exactly as the original.

/// Maps byte offsets to 0-based line (and, optionally, column) numbers.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line. Always begins with `0`.
    starts: Vec<usize>,
}

impl LineIndex {
    /// Build the index by scanning `text` once for newline (`\n`) boundaries.
    ///
    /// Both `\n` (Unix) and `\r\n` (Windows/CRLF) line endings are recognised.
    /// Line-start offsets always point to the first byte *after* the newline
    /// sequence, so byte offsets are preserved exactly as they appear in `text`.
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0usize];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        LineIndex { starts }
    }

    /// Returns the byte offset just past the last non-newline byte on `line`,
    /// i.e. the end of the line's visible content, excluding any trailing
    /// `\r` or `\n`. Used internally to make column arithmetic CRLF-safe.
    fn line_content_end(&self, text: &str, line: usize) -> usize {
        // The raw end is the byte before the next line's start (the `\n`), or
        // text.len() for the last line.
        let raw_end = self
            .starts
            .get(line + 1)
            .map(|&next| next.saturating_sub(1).min(text.len()))
            .unwrap_or(text.len());
        // Also strip a leading `\r` before the `\n` for CRLF sequences.
        if raw_end > self.starts[line]
            && text.as_bytes().get(raw_end.saturating_sub(1)) == Some(&b'\r')
        {
            raw_end - 1
        } else {
            raw_end
        }
    }

    /// 0-based source line containing byte offset `pos`.
    pub fn line_at(&self, pos: usize) -> usize {
        // partition_point yields the count of line-starts <= pos; subtract one for
        // the 0-based line number (there is always the offset-0 entry, so it's >=1).
        self.starts.partition_point(|&s| s <= pos) - 1
    }

    /// 0-based byte column of byte offset `pos` within its line.
    pub fn col_at(&self, pos: usize) -> usize {
        let line = self.line_at(pos);
        pos - self.starts[line]
    }

    /// 0-based `(line, column)` of byte offset `pos`, columns in bytes.
    pub fn line_col(&self, pos: usize) -> (usize, usize) {
        let line = self.line_at(pos);
        (line, pos - self.starts[line])
    }

    /// Byte offset of the start of 0-based `line`, if it exists.
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.starts.get(line).copied()
    }

    /// 0-based `(line, column)` of byte offset `pos`, with the column counted
    /// in `enc` code units (the LSP position encodings). `text` must be the
    /// same string this index was built from — the index stores only line
    /// starts, so the caller supplies the text for the column scan.
    ///
    /// `pos` is clamped to `text.len()` and rounded **down** to a UTF-8 char
    /// boundary: a diagnostic byte range can end mid-codepoint (e.g. an
    /// unterminated string/comment containing `é`/`°`/an emoji), and slicing
    /// at a non-boundary would panic — in an LSP that panic propagates out of
    /// the publish future and aborts the whole server (m1-lsp #132). Line
    /// starts always follow `\n`, so only the upper bound needs flooring.
    pub fn position_in(&self, text: &str, pos: usize, enc: PositionEncoding) -> (usize, usize) {
        let mut pos = pos.min(text.len());
        while pos > 0 && !text.is_char_boundary(pos) {
            pos -= 1;
        }
        let line = self.line_at(pos);
        // Clamp pos to the content end of the line, so a position pointing at
        // a trailing `\r` (CRLF) or `\n` is treated as end-of-content, not
        // as a byte within the line's visible text.
        let content_end = self.line_content_end(text, line);
        let effective_pos = pos.min(content_end);
        let slice = &text[self.starts[line]..effective_pos];
        let col = match enc {
            PositionEncoding::Utf8 => slice.len(),
            PositionEncoding::Utf16 => slice.chars().map(char::len_utf16).sum(),
        };
        (line, col)
    }

    /// Byte offset of 0-based `(line, column)` in `text`, with the column
    /// counted in `enc` code units — the inverse of [`Self::position_in`].
    /// `text` must be the same string this index was built from.
    ///
    /// Out-of-range positions clamp: a line past the end maps to `text.len()`,
    /// and a column past the end of its line (or landing inside a code point)
    /// stops at the last boundary that fits, never beyond the line.
    pub fn offset_in(
        &self,
        text: &str,
        line: usize,
        column: usize,
        enc: PositionEncoding,
    ) -> usize {
        let Some(&line_start) = self.starts.get(line) else {
            return text.len();
        };
        // The content end excludes trailing `\n` (and `\r` for CRLF) so that
        // an out-of-range column clamps to visible end-of-line rather than
        // crossing into the newline sequence or the next line
        // (LSP spec §3.16.1, issue #33).
        let line_end = self.line_content_end(text, line);
        let mut remaining = column;
        let mut off = line_start;
        for c in text[line_start..line_end].chars() {
            if remaining == 0 {
                break;
            }
            let units = match enc {
                PositionEncoding::Utf8 => c.len_utf8(),
                PositionEncoding::Utf16 => c.len_utf16(),
            };
            if units > remaining {
                break;
            }
            remaining -= units;
            off += c.len_utf8();
        }
        off
    }
}

/// Code-unit encoding for [`LineIndex::position_in`] / [`LineIndex::offset_in`]
/// columns — the two position encodings an LSP client can negotiate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    /// Columns in UTF-8 bytes.
    Utf8,
    /// Columns in UTF-16 code units (the LSP default).
    Utf16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.line_at(0), 0);
        assert_eq!(idx.line_at(4), 0);
        assert_eq!(idx.col_at(3), 3);
    }

    #[test]
    fn multi_line() {
        // "ab\ncd\ne" — line starts at bytes 0, 3, 6.
        let idx = LineIndex::new("ab\ncd\ne");
        assert_eq!(idx.line_at(0), 0); // 'a'
        assert_eq!(idx.line_at(2), 0); // '\n' belongs to its line
        assert_eq!(idx.line_at(3), 1); // 'c'
        assert_eq!(idx.line_at(5), 1); // '\n'
        assert_eq!(idx.line_at(6), 2); // 'e'
        assert_eq!(idx.line_col(4), (1, 1)); // 'd'
        assert_eq!(idx.line_start(2), Some(6));
        assert_eq!(idx.line_start(3), None);
    }

    #[test]
    fn multi_byte_chars() {
        // "é" is 2 bytes (0xC3 0xA9); "abc" then "édef" on line 1.
        let text = "abc\nédef";
        let idx = LineIndex::new(text);
        // 'd' is at byte offset 4 (a b c \n) + 2 (é) = 6.
        let d_off = text.find('d').unwrap();
        assert_eq!(d_off, 6);
        assert_eq!(idx.line_at(d_off), 1);
        // Column is measured in BYTES: é occupies bytes 0..2 on line 1, so 'd' is col 2.
        assert_eq!(idx.col_at(d_off), 2);
        assert_eq!(idx.line_col(d_off), (1, 2));
    }

    #[test]
    fn trailing_newline() {
        let idx = LineIndex::new("x\n");
        assert_eq!(idx.line_at(0), 0);
        // Offset past the final newline maps to the (empty) trailing line.
        assert_eq!(idx.line_at(2), 1);
    }

    // The encoding-aware conversions below were hoisted from m1-lsp's local
    // LineIndex copy (m1-lsp #265); their tests moved upstream with the code.

    #[test]
    fn position_in_ascii() {
        let s = "ab\ncde\n";
        let li = LineIndex::new(s);
        // 'a'@0 -> 0:0 ; 'c'@3 -> 1:0 ; 'e'@5 -> 1:2 ; end@7 -> 2:0
        assert_eq!(li.position_in(s, 0, PositionEncoding::Utf16), (0, 0));
        assert_eq!(li.position_in(s, 3, PositionEncoding::Utf16), (1, 0));
        assert_eq!(li.position_in(s, 5, PositionEncoding::Utf16), (1, 2));
        assert_eq!(li.position_in(s, 7, PositionEncoding::Utf16), (2, 0));
    }

    #[test]
    fn position_in_utf16_counts_code_units_not_bytes() {
        // "é" is 2 bytes in UTF-8, 1 UTF-16 code unit. "𝄞" is 4 bytes, 2 units.
        let s = "é𝄞x"; // bytes: 2 + 4 + 1 = 7
        let li = LineIndex::new(s);
        // byte 7 (end) -> column 1(é)+2(𝄞)+1(x) = 4 UTF-16 units
        assert_eq!(li.position_in(s, 7, PositionEncoding::Utf16), (0, 4));
        // same byte in UTF-8 encoding counts bytes -> column 7
        assert_eq!(li.position_in(s, 7, PositionEncoding::Utf8), (0, 7));
    }

    #[test]
    fn offset_in_round_trips() {
        let s = "ab\né𝄞x\n";
        let li = LineIndex::new(s);
        for enc in [PositionEncoding::Utf16, PositionEncoding::Utf8] {
            for b in [0usize, 1, 3, 5, 9, s.len()] {
                if !s.is_char_boundary(b) {
                    continue;
                }
                let (line, col) = li.position_in(s, b, enc);
                assert_eq!(li.offset_in(s, line, col, enc), b, "byte {b} via {enc:?}");
            }
        }
        // A line past the end clamps to text.len().
        assert_eq!(li.offset_in(s, 99, 0, PositionEncoding::Utf16), s.len());
    }

    #[test]
    fn position_in_empty_document() {
        let li = LineIndex::new("");
        assert_eq!(li.position_in("", 0, PositionEncoding::Utf16), (0, 0));
    }

    #[test]
    fn offset_in_out_of_range_column_clamps_to_end_of_line_not_next_line() {
        // Issue #33: an out-of-range column must clamp to end-of-line, not cross
        // the trailing `\n` into the next line (LSP spec §3.16.1). Previously
        // `line_end` included the newline, so a huge column consumed the `\n`
        // and `position_in` reported the *next* line.
        let s = "x\nsome other content";
        let li = LineIndex::new(s);
        // Column 100 on line 0 is way past the single 'x'; it must land at the
        // end of line 0 (byte 1, just before the '\n'), NOT byte 2 (line 1).
        let off = li.offset_in(s, 0, 100, PositionEncoding::Utf16);
        assert_eq!(off, 1, "out-of-range column must stop before the newline");
        // And the round-trip back must stay on line 0.
        assert_eq!(li.position_in(s, off, PositionEncoding::Utf16).0, 0);

        // Same for UTF-8 columns, and for a middle line with a real EOL.
        let off8 = li.offset_in(s, 0, 100, PositionEncoding::Utf8);
        assert_eq!(off8, 1);
        assert_eq!(li.position_in(s, off8, PositionEncoding::Utf8).0, 0);

        // Multi-line: an out-of-range column on a non-last line stops at that
        // line's EOL, never on the following line.
        let m = "ab\ncd\nef";
        let mi = LineIndex::new(m);
        let line1_eol = mi.offset_in(m, 1, 50, PositionEncoding::Utf16);
        assert_eq!(line1_eol, 5, "line 1 ends at byte 5 (before its '\\n')");
        assert_eq!(mi.position_in(m, line1_eol, PositionEncoding::Utf16).0, 1);
    }

    // ── CRLF tests ──────────────────────────────────────────────────────────

    #[test]
    fn crlf_line_starts_are_correct() {
        // "line1\r\nline2\r\nline3\r\n" — line starts must be at 0, 7, 14.
        let text = "line1\r\nline2\r\nline3\r\n";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_start(0), Some(0));
        assert_eq!(idx.line_start(1), Some(7));
        assert_eq!(idx.line_start(2), Some(14));
        assert_eq!(idx.line_start(3), Some(21)); // trailing empty line after last \n
    }

    #[test]
    fn crlf_line_at_is_correct() {
        // "a\r\nb\r\n": 'a'@0, '\r'@1, '\n'@2, 'b'@3, '\r'@4, '\n'@5, ""@6
        let text = "a\r\nb\r\n";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_at(0), 0, "'a' is on line 0");
        assert_eq!(idx.line_at(1), 0, "'\\r' is still on line 0");
        assert_eq!(idx.line_at(2), 0, "'\\n' is on line 0");
        assert_eq!(idx.line_at(3), 1, "'b' is on line 1");
        assert_eq!(idx.line_at(5), 1, "'\\n' of second CRLF is on line 1");
        assert_eq!(idx.line_at(6), 2, "empty trailing line");
    }

    #[test]
    fn crlf_position_in_excludes_cr_from_column() {
        // "a\r\nb\r\n": 'b' is at byte 3; its column must be 0, not 1.
        // A position pointing at the '\r' (byte 1) should give column 1 (end
        // of visible content "a"), identical to pointing past "a" but not at
        // the CR itself.
        let text = "a\r\nb\r\n";
        let li = LineIndex::new(text);
        // 'a' at byte 0 → (0, 0)
        assert_eq!(li.position_in(text, 0, PositionEncoding::Utf8), (0, 0));
        // '\r' at byte 1 — must not count \r as line content; column = 1 (length of "a")
        assert_eq!(
            li.position_in(text, 1, PositionEncoding::Utf8),
            (0, 1),
            "\\r should be treated as end-of-content, column = len(\"a\") = 1"
        );
        // '\n' at byte 2 — same: column = 1
        assert_eq!(li.position_in(text, 2, PositionEncoding::Utf8), (0, 1));
        // 'b' at byte 3 → (1, 0)
        assert_eq!(li.position_in(text, 3, PositionEncoding::Utf8), (1, 0));
    }

    #[test]
    fn crlf_offset_in_does_not_include_cr() {
        // offset_in with a column past end-of-content must stop before \r, not at \r.
        let text = "abc\r\ndef\r\n";
        let li = LineIndex::new(text);
        // Line 0 visible content is "abc" (bytes 0..3); \r is at byte 3, \n at byte 4.
        // An out-of-range column must clamp to byte 3 (just past 'c'), not byte 4 (\n).
        let off = li.offset_in(text, 0, 999, PositionEncoding::Utf8);
        assert_eq!(
            off, 3,
            "out-of-range column should stop after 'c', before \\r"
        );
        // Round-trip: position_in of that offset must stay on line 0.
        assert_eq!(li.position_in(text, off, PositionEncoding::Utf8).0, 0);
    }

    #[test]
    fn crlf_col_at_content_bytes() {
        // col_at measures raw bytes from line start; the CRLF fix applies to
        // position_in/offset_in — col_at itself reports the byte offset as-is.
        let text = "ab\r\ncd\r\n";
        let idx = LineIndex::new(text);
        assert_eq!(idx.col_at(0), 0); // 'a'
        assert_eq!(idx.col_at(1), 1); // 'b'
        assert_eq!(idx.col_at(4), 0); // 'c' on line 1
        assert_eq!(idx.col_at(5), 1); // 'd' on line 1
    }

    // ── end CRLF tests ──────────────────────────────────────────────────────

    #[test]
    fn position_in_mid_codepoint_byte_does_not_panic() {
        // m1-lsp #132: a diagnostic byte range can end inside a multibyte char
        // (e.g. an unterminated comment containing `𝄞`). `position_in` must
        // clamp to a char boundary instead of panicking.
        let s = "/* é 𝄞"; // '𝄞' occupies bytes 6..10
        let li = LineIndex::new(s);
        // Byte 7 is inside the 4-byte '𝄞' — must behave like byte 6, not panic.
        assert_eq!(
            li.position_in(s, 7, PositionEncoding::Utf16),
            li.position_in(s, 6, PositionEncoding::Utf16)
        );
        assert_eq!(li.position_in(s, 7, PositionEncoding::Utf16), (0, 5));
        // Past-end offsets must also be safe.
        let _ = li.position_in(s, s.len(), PositionEncoding::Utf16);
        let _ = li.position_in(s, 9999, PositionEncoding::Utf8);
    }
}
