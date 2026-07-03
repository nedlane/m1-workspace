//! Tolerant text decoding for MoTeC's files (`.m1prj` / `.m1cfg` / `.m1dbc` /
//! `.m1scr`).
//!
//! MoTeC writes its XML files declared `<?xml version="1.0"?>` — no `encoding`
//! attribute, which per the XML spec means UTF-8 — but emits **Windows-1252**
//! bytes for non-ASCII characters, and the script files (`.m1scr`) follow the
//! same convention. A CAN signal whose unit is a yaw rate, for example, stores
//! `°/s` with the degree sign as the single byte `0xB0`, which is not valid
//! UTF-8. A strict `read_to_string` of such a file errors with "stream did not
//! contain valid UTF-8", and that failure used to abort whole tool runs (a
//! project-model build, a format pass, a lint batch).
//!
//! We therefore read the raw bytes and decode tolerantly: try UTF-8 first (the
//! common case, and what the declaration claims), and on failure fall back to
//! Windows-1252 — a superset of Latin-1 — which never fails and recovers `°`,
//! `±`, `µ`, etc. exactly.
//!
//! This decoder began in `m1-typecheck` (v0.16.2); it lives here so every tool
//! in the toolchain shares one implementation. Tools that *write* files back
//! (m1-project) use [`decode_with_encoding`] + [`encode`] to round-trip a file
//! in its original encoding instead of silently transcoding it to UTF-8.

use std::io;
use std::path::Path;

/// The byte encoding a MoTeC file was decoded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Valid UTF-8 (the common case, and what the XML declaration claims).
    Utf8,
    /// Windows-1252 (what MoTeC actually emits for non-ASCII bytes).
    Windows1252,
}

/// Upper bound on the size of a MoTeC text file the toolchain will read into
/// memory. Real M1 files are tens-to-hundreds of KB (the largest in the
/// reference corpus is a ~450 KB `Project.m1prj`); 64 MiB is ample headroom. A
/// larger file fails fast with `InvalidData` instead of attempting a
/// multi-gigabyte allocation — an OOM/DoS vector for batch runs and the
/// long-lived LSP (m1-workspace#8).
pub const MAX_TEXT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Read a file as text, decoding UTF-8 with a Windows-1252 fallback. General
/// helper for any MoTeC text file (`.m1scr`, `.m1prj`, `.m1cfg`, `.m1dbc`).
/// Files larger than [`MAX_TEXT_FILE_BYTES`] are rejected.
pub fn read_text(path: &Path) -> io::Result<String> {
    read_text_capped(path, MAX_TEXT_FILE_BYTES)
}

/// Like [`read_text`] but with a caller-supplied size cap (in bytes). A file
/// larger than `max_bytes` returns `ErrorKind::InvalidData` after a cheap
/// metadata check, before any large allocation.
pub fn read_text_capped(path: &Path, max_bytes: u64) -> io::Result<String> {
    Ok(decode(read_bytes_capped(path, max_bytes)?))
}

/// Read a MoTeC XML file as text. Currently identical to [`read_text`]; kept as
/// a named helper for clarity at the XML reads (`.m1prj`/`.m1cfg`/`.m1dbc`).
pub fn read_motec_xml(path: &Path) -> io::Result<String> {
    read_text(path)
}

/// Read a file as text, also reporting the encoding it was decoded from, so a
/// writer can round-trip it via [`encode`] in the same encoding. Subject to the
/// same [`MAX_TEXT_FILE_BYTES`] size cap as [`read_text`].
pub fn read_text_with_encoding(path: &Path) -> io::Result<(String, Encoding)> {
    Ok(decode_with_encoding(read_bytes_capped(
        path,
        MAX_TEXT_FILE_BYTES,
    )?))
}

/// Read a file's raw bytes, rejecting anything larger than `max_bytes` via a
/// metadata pre-check so an oversized input fails fast instead of OOMing.
fn read_bytes_capped(path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
    let len = std::fs::metadata(path)?.len();
    if len > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is {len} bytes, exceeding the {max_bytes}-byte read limit"),
        ));
    }
    std::fs::read(path)
}

/// Decode bytes as UTF-8, falling back to Windows-1252 if they are not valid
/// UTF-8. Pure, so it is unit-testable without touching the filesystem.
pub fn decode(bytes: Vec<u8>) -> String {
    decode_with_encoding(bytes).0
}

/// Like [`decode`] but also reports which encoding was used. A `.m1prj` editor
/// (m1-project) needs this to re-encode on write-back instead of transcoding a
/// Windows-1252 file to UTF-8 behind MoTeC's back.
pub fn decode_with_encoding(bytes: Vec<u8>) -> (String, Encoding) {
    // UTF-16 with a byte-order mark (a Notepad "Unicode" / "Unicode big endian"
    // save): decode it properly instead of mojibaking every second byte through
    // the Windows-1252 fallback below. M1 files are UTF-8 / Windows-1252, so a
    // UTF-16 source is already non-standard — we normalise it to a correct
    // Unicode string (reported as UTF-8, so a write-back lands in a standard
    // encoding) rather than preserving the UTF-16 form.
    let utf16_le =
        bytes.starts_with(&[0xFF, 0xFE]) && !bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]);
    let utf16_be = bytes.starts_with(&[0xFE, 0xFF]);
    if utf16_le || utf16_be {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| {
                if utf16_le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        return (String::from_utf16_lossy(&units), Encoding::Utf8);
    }

    // Strip a leading UTF-8 BOM (EF BB BF) so the decoded text begins with the
    // document's first real character, not U+FEFF — a stray BOM shifts
    // diagnostic columns and breaks strict leading-token checks, and on the
    // Windows-1252 fallback below would otherwise mojibake to `ï»¿`
    // (m1-workspace#9). Copies only when a BOM is actually present.
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes[3..].to_vec()
    } else {
        bytes
    };
    match String::from_utf8(bytes) {
        Ok(s) => (s, Encoding::Utf8),
        // `into_bytes` reuses the original buffer (no UTF-8 was produced).
        Err(e) => (cp1252_to_string(&e.into_bytes()), Encoding::Windows1252),
    }
}

/// Encode a string to bytes in `encoding`, the inverse of [`decode_with_encoding`].
///
/// UTF-8 is a straight copy. Windows-1252 maps each `char` to its single 1252
/// byte. Characters absent from Windows-1252 are written as `?` (`0x3F`) — these
/// cannot have come from a 1252 decode, so this is lossy only for characters a
/// caller newly *inserted* into a 1252-sourced file (e.g. an emoji in a unit).
pub fn encode(s: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => s.as_bytes().to_vec(),
        Encoding::Windows1252 => s.chars().map(char_to_cp1252).collect(),
    }
}

/// The characters a target encoding cannot represent, returned by
/// [`encode_checked`] so a writer can refuse a lossy write instead of silently
/// substituting `'?'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LossyEncoding {
    /// The distinct un-representable characters, in first-seen order.
    pub unrepresentable: Vec<char>,
}

impl std::fmt::Display for LossyEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list: Vec<String> = self
            .unrepresentable
            .iter()
            .map(|c| format!("{c:?} (U+{:04X})", *c as u32))
            .collect();
        write!(
            f,
            "{} character(s) cannot be represented in Windows-1252: {}",
            self.unrepresentable.len(),
            list.join(", ")
        )
    }
}

impl std::error::Error for LossyEncoding {}

/// Like [`encode`], but instead of silently substituting `'?'` for characters
/// the target encoding cannot represent, returns [`LossyEncoding`] listing them.
///
/// UTF-8 can represent every `char`, so it never fails. Use this on a
/// write-back path (e.g. m1-project rewriting a Windows-1252 `Project.m1prj`) so
/// a user-supplied value outside Windows-1252 — an ohm `Ω`, an arrow, CJK — is
/// reported and the write refused, rather than persisted as `'?'`
/// (m1-workspace#6).
pub fn encode_checked(s: &str, encoding: Encoding) -> Result<Vec<u8>, LossyEncoding> {
    match encoding {
        Encoding::Utf8 => Ok(s.as_bytes().to_vec()),
        Encoding::Windows1252 => {
            let mut out = Vec::with_capacity(s.len());
            let mut bad: Vec<char> = Vec::new();
            for c in s.chars() {
                let b = char_to_cp1252(c);
                // `char_to_cp1252` returns `b'?'` both for a literal '?' and for
                // any character it cannot represent; only the latter is lossy.
                if b == b'?' && c != '?' {
                    if !bad.contains(&c) {
                        bad.push(c);
                    }
                } else {
                    out.push(b);
                }
            }
            if bad.is_empty() {
                Ok(out)
            } else {
                Err(LossyEncoding {
                    unrepresentable: bad,
                })
            }
        }
    }
}

fn cp1252_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| cp1252_char(b)).collect()
}

/// Map a single Windows-1252 byte to its Unicode scalar. `0x00–0x7F` and
/// `0xA0–0xFF` are identical to Latin-1 (the first 256 Unicode code points), so
/// only the `0x80–0x9F` "C1" range needs an explicit table. The five bytes
/// undefined in CP1252 map to U+FFFD.
fn cp1252_char(b: u8) -> char {
    match b {
        0x80 => '\u{20AC}', // €
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}', // ™
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        // Undefined in Windows-1252.
        0x81 | 0x8D | 0x8F | 0x90 | 0x9D => '\u{FFFD}',
        // 0x00–0x7F (ASCII) and 0xA0–0xFF (Latin-1) are 1:1 with Unicode.
        _ => b as char,
    }
}

/// Inverse of [`cp1252_char`]: map a Unicode scalar to its single Windows-1252
/// byte, or `?` (`0x3F`) for characters Windows-1252 cannot represent.
fn char_to_cp1252(c: char) -> u8 {
    match c as u32 {
        // ASCII and Latin-1 are 1:1 with the low/high byte ranges.
        0x00..=0x7F | 0xA0..=0xFF => c as u8,
        // The CP1252 "C1" range (the only bytes that are not Latin-1).
        0x20AC => 0x80,
        0x201A => 0x82,
        0x0192 => 0x83,
        0x201E => 0x84,
        0x2026 => 0x85,
        0x2020 => 0x86,
        0x2021 => 0x87,
        0x02C6 => 0x88,
        0x2030 => 0x89,
        0x0160 => 0x8A,
        0x2039 => 0x8B,
        0x0152 => 0x8C,
        0x017D => 0x8E,
        0x2018 => 0x91,
        0x2019 => 0x92,
        0x201C => 0x93,
        0x201D => 0x94,
        0x2022 => 0x95,
        0x2013 => 0x96,
        0x2014 => 0x97,
        0x02DC => 0x98,
        0x2122 => 0x99,
        0x0161 => 0x9A,
        0x203A => 0x9B,
        0x0153 => 0x9C,
        0x017E => 0x9E,
        0x0178 => 0x9F,
        _ => b'?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_passes_through_unchanged() {
        let (s, enc) = decode_with_encoding("Qty=\"°/s\"".as_bytes().to_vec());
        assert_eq!(s, "Qty=\"°/s\"");
        assert_eq!(enc, Encoding::Utf8);
    }

    #[test]
    fn utf16_le_bom_is_decoded_not_mojibaked() {
        // A Notepad "Unicode" save: UTF-16 LE BOM + "A°" as u16 code units.
        let mut bytes = vec![0xFF, 0xFE]; // BOM
        bytes.extend_from_slice(&[0x41, 0x00]); // 'A'
        bytes.extend_from_slice(&[0xB0, 0x00]); // '°'
        let (s, enc) = decode_with_encoding(bytes);
        assert_eq!(s, "A°");
        assert_eq!(enc, Encoding::Utf8);
    }

    #[test]
    fn utf16_be_bom_is_decoded() {
        let mut bytes = vec![0xFE, 0xFF]; // BE BOM
        bytes.extend_from_slice(&[0x00, 0x41]); // 'A'
        bytes.extend_from_slice(&[0x00, 0xB0]); // '°'
        let (s, enc) = decode_with_encoding(bytes);
        assert_eq!(s, "A°");
        assert_eq!(enc, Encoding::Utf8);
    }

    #[test]
    fn windows_1252_degree_sign_is_recovered() {
        // MoTeC stores `°` as the single CP1252 byte 0xB0, which is invalid UTF-8.
        let mut bytes = b"Qty=\"".to_vec();
        bytes.push(0xB0); // °
        bytes.extend_from_slice(b"/s\"");
        let (s, enc) = decode_with_encoding(bytes);
        assert_eq!(s, "Qty=\"°/s\"");
        assert_eq!(enc, Encoding::Windows1252);
    }

    #[test]
    fn windows_1252_c1_range_and_latin1() {
        // ± (0xB1) and µ (0xB5) are Latin-1; € (0x80) and ™ (0x99) are CP1252 C1.
        let bytes = vec![0xB1, 0xB5, 0x80, 0x99];
        assert_eq!(decode(bytes), "±µ€™");
    }

    #[test]
    fn encode_utf8_is_a_straight_copy() {
        assert_eq!(encode("°/s", Encoding::Utf8), "°/s".as_bytes());
    }

    #[test]
    fn encode_windows_1252_degree_round_trips() {
        // The byte that started it all: 0xB0 -> '°' -> 0xB0.
        let bytes = vec![b'Q', 0xB0, b'/', b's'];
        let (s, enc) = decode_with_encoding(bytes.clone());
        assert_eq!(enc, Encoding::Windows1252);
        assert_eq!(encode(&s, enc), bytes);
    }

    #[test]
    fn encode_windows_1252_c1_round_trips() {
        // C1-range bytes (€, ™, “ ”) must survive a decode→encode round-trip.
        let bytes = vec![0x80, 0x99, 0x93, 0x94, 0xB1, 0xB5];
        let (s, _) = decode_with_encoding(bytes.clone());
        assert_eq!(encode(&s, Encoding::Windows1252), bytes);
    }

    #[test]
    fn encode_windows_1252_unrepresentable_char_becomes_question_mark() {
        // An emoji can't exist in a 1252 file; encoding it is defined as '?'.
        assert_eq!(encode("a🚗b", Encoding::Windows1252), b"a?b");
    }

    #[test]
    fn utf8_bom_is_stripped() {
        // #9: a leading UTF-8 BOM must not survive into the decoded string.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<Project/>");
        let (s, enc) = decode_with_encoding(bytes);
        assert_eq!(enc, Encoding::Utf8);
        assert_eq!(s, "<Project/>");
        assert!(!s.starts_with('\u{FEFF}'));
    }

    #[test]
    fn encode_checked_reports_unrepresentable_char() {
        // #6: ohm Ω (U+03A9) is outside Windows-1252; it must be reported, not
        // silently turned into '?'.
        let err = encode_checked("12 Ω", Encoding::Windows1252).unwrap_err();
        assert_eq!(err.unrepresentable, vec!['\u{03A9}']);
    }

    #[test]
    fn encode_checked_allows_latin1_and_literal_question_mark() {
        // Everyday CP1252 symbols and a *literal* '?' must encode without error.
        let ok = encode_checked("°±µ? ok", Encoding::Windows1252).unwrap();
        assert_eq!(ok, encode("°±µ? ok", Encoding::Windows1252));
    }

    #[test]
    fn encode_checked_utf8_never_fails() {
        assert_eq!(
            encode_checked("a🚗Ω", Encoding::Utf8).unwrap(),
            "a🚗Ω".as_bytes()
        );
    }

    #[test]
    fn read_text_capped_rejects_oversized_file() {
        // #8: a file larger than the cap fails fast (no large allocation).
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("big.m1scr");
        std::fs::write(&p, vec![b'a'; 100]).unwrap();
        let err = read_text_capped(&p, 10).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        // A cap above the file size reads normally.
        assert_eq!(read_text_capped(&p, 1000).unwrap().len(), 100);
    }
}
