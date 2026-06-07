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
}
