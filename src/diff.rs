//! Line-level unified diff used by the CLI's `--diff` output.
//!
//! A real LCS-based unified diff (grouped into hunks with surrounding context),
//! not a positional line-pairing — see [`unified_diff`] (#79).

/// One step of the line-level edit script between two files.
#[derive(Clone, Copy, PartialEq)]
pub enum Op {
    Equal,
    Delete,
    Insert,
}

/// Split `s` into lines in a way that preserves the trailing-newline
/// distinction. `str::lines()` normalises `"a\n"` and `"a"` to the same
/// `["a"]`, making files that differ *only* in a trailing newline compare as
/// identical. Instead we split on `\n` and strip any trailing `\r` from each
/// segment, handling both Unix (`\n`) and Windows (`\r\n`) line endings.
///
/// The trailing `""` produced by a final `\n` is kept as a real element
/// (representing the empty line *after* the last `\n`), while a string with
/// no trailing newline has no such element. The one-extra-empty-element
/// convention matches what `str::split` already does — no special-casing
/// needed at call sites.
fn split_lines(s: &str) -> Vec<&str> {
    s.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Build a real unified diff between `original` and `formatted`, grouping edits
/// into hunks with surrounding context — not a positional line-pairing, which
/// reports every line after an insertion as changed (#79). Returns the empty
/// string when the inputs are identical (no `--- / +++` header, no hunks).
pub fn unified_diff(name: &str, original: &str, formatted: &str) -> String {
    const CONTEXT: usize = 3;
    let a: Vec<&str> = split_lines(original);
    let b: Vec<&str> = split_lines(formatted);
    let script = edit_script(&a, &b);
    if script.iter().all(|(op, _, _)| *op == Op::Equal) {
        return String::new();
    }

    // Group consecutive non-equal ops (plus up to CONTEXT equal lines on each
    // side) into hunks.
    let mut out = String::new();
    out.push_str(&format!("--- {name} (original)\n"));
    out.push_str(&format!("+++ {name} (formatted)\n"));

    let mut i = 0;
    while i < script.len() {
        if script[i].0 == Op::Equal {
            i += 1;
            continue;
        }
        // Start of a change run: back up CONTEXT equal lines for leading context.
        let mut start = i;
        let mut ctx = 0;
        while start > 0 && script[start - 1].0 == Op::Equal && ctx < CONTEXT {
            start -= 1;
            ctx += 1;
        }
        // Extend to the end of the change run, absorbing gaps of <= 2*CONTEXT
        // equal lines so nearby changes share one hunk, then add trailing context.
        let mut end = i;
        while end < script.len() {
            if script[end].0 != Op::Equal {
                end += 1;
                continue;
            }
            // Count the run of equal lines starting at `end`.
            let mut run = end;
            while run < script.len() && script[run].0 == Op::Equal {
                run += 1;
            }
            let run_len = run - end;
            let more_changes = run < script.len();
            if more_changes && run_len <= 2 * CONTEXT {
                end = run; // bridge the gap into the same hunk
            } else {
                end += run_len.min(CONTEXT); // trailing context, then close
                break;
            }
        }

        // Hunk header line ranges (1-based; 0,0 when a side is empty).
        let (mut a_start, mut b_start) = (0usize, 0usize);
        let (mut a_count, mut b_count) = (0usize, 0usize);
        for (op, ai, bi) in &script[start..end] {
            match op {
                Op::Equal => {
                    if a_count == 0 {
                        a_start = ai + 1;
                    }
                    if b_count == 0 {
                        b_start = bi + 1;
                    }
                    a_count += 1;
                    b_count += 1;
                }
                Op::Delete => {
                    if a_count == 0 {
                        a_start = ai + 1;
                    }
                    a_count += 1;
                }
                Op::Insert => {
                    if b_count == 0 {
                        b_start = bi + 1;
                    }
                    b_count += 1;
                }
            }
        }
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            a_start, a_count, b_start, b_count
        ));
        for (op, ai, bi) in &script[start..end] {
            match op {
                Op::Equal => out.push_str(&format!(" {}\n", a[*ai])),
                Op::Delete => out.push_str(&format!("-{}\n", a[*ai])),
                Op::Insert => out.push_str(&format!("+{}\n", b[*bi])),
            }
        }
        i = end;
    }
    out
}

/// The line-level LCS edit script: a sequence of `(Op, a_index, b_index)`. For
/// `Equal` both indices are meaningful; for `Delete` only `a_index`, for
/// `Insert` only `b_index` (the other is the last consumed index, unused).
fn edit_script(a: &[&str], b: &[&str]) -> Vec<(Op, usize, usize)> {
    let (n, m) = (a.len(), b.len());
    // LCS length DP table.
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    // Walk the table to recover the edit script.
    let mut script = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            script.push((Op::Equal, i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            script.push((Op::Delete, i, j));
            i += 1;
        } else {
            script.push((Op::Insert, i, j));
            j += 1;
        }
    }
    while i < n {
        script.push((Op::Delete, i, j));
        i += 1;
    }
    while j < m {
        script.push((Op::Insert, i, j));
        j += 1;
    }
    script
}

#[cfg(test)]
mod diff_tests {
    use super::*;

    #[test]
    fn inserted_line_keeps_following_lines_as_context() {
        // #79: a real unified diff. Inserting one line must not report every
        // following identical line as changed; they stay as ` ` context lines,
        // and a `@@` hunk header frames the change.
        let original = "a\nb\nc\n";
        let formatted = "a\nX\nb\nc\n";
        let d = unified_diff("demo.m1scr", original, formatted);
        assert!(d.contains("@@"), "expected a hunk header:\n{d}");
        assert!(d.contains("+X"), "expected the inserted line:\n{d}");
        // `b` and `c` are unchanged: they appear as context, never as -/+ pairs.
        assert!(d.contains(" b"), "b should be a context line:\n{d}");
        assert!(d.contains(" c"), "c should be a context line:\n{d}");
        assert!(!d.contains("-b"), "b must not be reported as deleted:\n{d}");
        assert!(!d.contains("-c"), "c must not be reported as deleted:\n{d}");
    }

    #[test]
    fn identical_inputs_produce_no_hunks() {
        let d = unified_diff("demo.m1scr", "a\nb\n", "a\nb\n");
        assert!(!d.contains("@@"), "no changes -> no hunks:\n{d}");
    }

    #[test]
    fn trailing_newline_distinction_is_preserved() {
        // Files that differ ONLY in a trailing newline must not compare as
        // identical. str::lines() normalises "a\n" and "a" to the same
        // Vec<&str> (["a"]), masking the difference — relevant because
        // `final_blank_line` is a first-class [format] knob that can cause
        // exactly this kind of change.
        let with_newline = "a\nb\n";
        let without_newline = "a\nb";
        let d = unified_diff("demo.m1scr", with_newline, without_newline);
        assert!(
            !d.is_empty(),
            "files differing only in trailing newline must produce a non-empty diff:\n{d}"
        );
        assert!(d.contains("@@"), "expected a hunk header:\n{d}");

        // Also verify the extra-blank-line case while we're here.
        let without_extra = "a\n";
        let with_extra = "a\n\n";
        let d2 = unified_diff("demo.m1scr", without_extra, with_extra);
        assert!(
            !d2.is_empty(),
            "files differing only in trailing blank line must produce a non-empty diff:\n{d2}"
        );
    }

    #[test]
    fn changed_line_shown_as_delete_then_insert() {
        let d = unified_diff("demo.m1scr", "a\nb\nc\n", "a\nB\nc\n");
        assert!(d.contains("-b"), "{d}");
        assert!(d.contains("+B"), "{d}");
        assert!(
            d.contains(" a") && d.contains(" c"),
            "context around change:\n{d}"
        );
    }

    // ── CRLF tests ──────────────────────────────────────────────────────────

    #[test]
    fn crlf_input_produces_no_stray_cr_in_diff() {
        // CRLF files must diff cleanly: the \r must not appear in the output.
        let original = "a\r\nb\r\nc\r\n";
        let formatted = "a\r\nB\r\nc\r\n";
        let d = unified_diff("demo.m1scr", original, formatted);
        assert!(!d.contains('\r'), "stray \\r in diff output:\n{d:?}");
        assert!(d.contains("-b"), "expected deletion of 'b':\n{d}");
        assert!(d.contains("+B"), "expected insertion of 'B':\n{d}");
    }

    #[test]
    fn crlf_and_lf_identical_content_compare_equal() {
        // "a\r\nb\r\n" and "a\nb\n" have the same logical content line-by-line;
        // after CRLF normalisation they should produce no diff.
        let crlf = "a\r\nb\r\n";
        let lf = "a\nb\n";
        let d = unified_diff("demo.m1scr", crlf, lf);
        assert!(
            d.is_empty(),
            "CRLF and LF with same content should diff as identical:\n{d}"
        );
    }

    #[test]
    fn crlf_context_lines_have_no_stray_cr() {
        // Context lines (unchanged lines surrounding a change) must also be
        // free of trailing \r when the input uses CRLF.
        let original = "ctx1\r\nctx2\r\nchanged\r\nctx3\r\nctx4\r\n";
        let formatted = "ctx1\r\nctx2\r\nCHANGED\r\nctx3\r\nctx4\r\n";
        let d = unified_diff("demo.m1scr", original, formatted);
        assert!(!d.contains('\r'), "stray \\r in context lines:\n{d:?}");
        // Context lines should be present without \r.
        assert!(
            d.contains(" ctx1") || d.contains(" ctx2"),
            "context missing:\n{d}"
        );
    }

    // ── end CRLF tests ──────────────────────────────────────────────────────
}
