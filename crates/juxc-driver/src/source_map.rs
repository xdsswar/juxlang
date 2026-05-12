//! Map emitted-Rust line numbers back to original `.jux` source
//! locations using the `// JUX:file:line:col` markers the backend
//! sprinkles into its output.
//!
//! When `cargo build` fails on the emitted crate, rustc reports
//! errors anchored at emitted-Rust lines (e.g. `--> src/main.rs:42:9`).
//! Those lines are auto-generated and don't match anything in the
//! user's `.jux` source. This module walks the emitted source for
//! `// JUX:` markers, builds an `(emitted-line → jux-location)` map,
//! and rewrites rustc's stderr so error arrows point at the original
//! Jux site.
//!
//! Crude (string-walk + regex-ish parsing, not real DWARF) but enough
//! to close the audit's Tier 2.2 UX gap until proper debuginfo lands.

/// One marker found inside the emitted Rust crate. Used to back-map
/// a rustc error anchor at `emitted-line` to a Jux source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkerEntry {
    /// 1-based line of the `// JUX:` comment within the emitted
    /// `main.rs`.
    pub emitted_line: u32,
    /// Path to the original `.jux` file as the backend wrote it.
    /// Mirrors what `SourceFile::path()` produced (which is whatever
    /// the driver passed to `lex` — typically an absolute or
    /// project-relative path).
    pub jux_path: String,
    /// 1-based line within the `.jux` file.
    pub jux_line: u32,
    /// 1-based column within the `.jux` file.
    pub jux_col: u32,
}

/// All the markers found in one emitted source file, sorted by
/// `emitted_line`. Provides a binary-search-style "what was the
/// nearest marker at or before this Rust line?" lookup.
#[derive(Debug, Default)]
pub(crate) struct MarkerMap {
    entries: Vec<MarkerEntry>,
}

impl MarkerMap {
    /// Scan `rust_src` line-by-line for `// JUX:path:line:col`
    /// markers. Lines without a marker are skipped silently.
    ///
    /// The path may contain colons (Windows: `C:\Users\…`), so we
    /// parse from the right: first the column, then the line, then
    /// whatever's left is the path. Any line that fails to parse
    /// cleanly is dropped (better to drop a malformed marker than
    /// to mis-attribute an error).
    pub(crate) fn from_emitted_source(rust_src: &str) -> Self {
        let mut entries = Vec::new();
        for (i, line) in rust_src.lines().enumerate() {
            let emitted_line = (i + 1) as u32;
            let stripped = line.trim_start();
            let Some(rest) = stripped.strip_prefix("// JUX:") else { continue };
            // `rest` is `path:line:col`. Parse from the right.
            let Some((before_col, col_str)) = rest.rsplit_once(':') else { continue };
            let Some((path, line_str)) = before_col.rsplit_once(':') else { continue };
            let Ok(jux_line) = line_str.parse::<u32>() else { continue };
            let Ok(jux_col) = col_str.parse::<u32>() else { continue };
            entries.push(MarkerEntry {
                emitted_line,
                jux_path: path.to_string(),
                jux_line,
                jux_col,
            });
        }
        Self { entries }
    }

    /// Find the most recent marker at or before `rust_line`. Returns
    /// `None` when the line precedes every marker (e.g. the header
    /// comment block at the top of the emitted file).
    pub(crate) fn lookup(&self, rust_line: u32) -> Option<&MarkerEntry> {
        // Binary search would be cleaner, but the linear-reverse walk
        // is fine: marker counts are small (one per stmt + decl in
        // practice) and we run this at most once per rustc error
        // line.
        self.entries.iter().rev().find(|e| e.emitted_line <= rust_line)
    }

    /// True if the map has no entries — used by [`rewrite_rustc_output`]
    /// as an early-exit so marker-less crates (e.g. ones lowered via
    /// `lower_with_types`) get their stderr passed through unchanged.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Rewrite rustc's stderr so file-anchor lines (`--> src/main.rs:N:M`)
/// point at the original `.jux` source location instead of the
/// emitted Rust. The snippet lines below the arrow stay as-is — they
/// show the emitted Rust verbatim, which is what rustc actually saw
/// — but the arrow + the new path/line/col let the user open the
/// `.jux` file directly.
///
/// Format of each rewritten arrow:
///
/// ```text
///   --> jux/path.jux:5:9  (= src/main.rs:14:9)
/// ```
///
/// The parenthetical preserves the emitted-Rust location so users
/// who want to inspect the lowered code (e.g. to file a juxc bug)
/// can still find it. When no marker covers the line, the original
/// arrow passes through unchanged.
///
/// `--> ` is the only pattern rewritten; rustc also emits secondary
/// locations on `note:` / `help:` lines but those use varied
/// shapes — a future turn can extend coverage. The primary error
/// anchor being correct is the practical win.
pub(crate) fn rewrite_rustc_output(stderr: &str, map: &MarkerMap) -> String {
    if map.is_empty() {
        return stderr.to_string();
    }
    let mut out = String::with_capacity(stderr.len());
    let mut first = true;
    for line in stderr.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if let Some(rewritten) = rewrite_arrow_line(line, map) {
            out.push_str(&rewritten);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Try to rewrite a single line as a `--> path:line:col` anchor.
/// Returns `None` when the line isn't an arrow line or when the
/// referenced emitted-line has no preceding marker.
fn rewrite_arrow_line(line: &str, map: &MarkerMap) -> Option<String> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    let rest = trimmed.strip_prefix("--> ")?;
    // `rest` is `path:LINE:COL`. The path can contain `:` on
    // Windows (`C:\…`), so parse from the right just like the
    // marker scanner does.
    let (before_col, col_str) = rest.rsplit_once(':')?;
    let (rust_path, line_str) = before_col.rsplit_once(':')?;
    let rust_line: u32 = line_str.parse().ok()?;
    let entry = map.lookup(rust_line)?;
    Some(format!(
        "{indent}--> {jux_path}:{jline}:{jcol}  (= {rust_path}:{rust_line}:{col_str})",
        jux_path = entry.jux_path,
        jline = entry.jux_line,
        jcol = entry.jux_col,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `// JUX:path:5:9` marker on emitted line N maps line N to
    /// `(path, 5, 9)`. Lines without markers stay unmapped.
    #[test]
    fn marker_map_parses_basic_markers() {
        let src = "fn main() {\n// JUX:test.jux:5:9\n    let x = 1;\n}";
        let map = MarkerMap::from_emitted_source(src);
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0].emitted_line, 2);
        assert_eq!(map.entries[0].jux_path, "test.jux");
        assert_eq!(map.entries[0].jux_line, 5);
        assert_eq!(map.entries[0].jux_col, 9);
    }

    /// Indented markers also parse — backend emits markers at the
    /// statement's indent depth.
    #[test]
    fn marker_map_parses_indented_markers() {
        let src = "    // JUX:test.jux:3:5\n    let y = 2;";
        let map = MarkerMap::from_emitted_source(src);
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0].jux_line, 3);
    }

    /// Windows-style paths with a drive-letter colon round-trip via
    /// the rsplit-from-right parsing.
    #[test]
    fn marker_map_handles_windows_paths() {
        let src = r"// JUX:C:\Users\dev\src\hi.jux:7:12";
        let map = MarkerMap::from_emitted_source(src);
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0].jux_path, r"C:\Users\dev\src\hi.jux");
        assert_eq!(map.entries[0].jux_line, 7);
        assert_eq!(map.entries[0].jux_col, 12);
    }

    /// `lookup(N)` returns the most recent marker at or before N.
    #[test]
    fn lookup_returns_nearest_preceding_marker() {
        let src = "// JUX:a.jux:1:1\nfn main() {\n// JUX:a.jux:2:5\nlet x = 1;\nlet y = 2;\n}";
        let map = MarkerMap::from_emitted_source(src);
        // Line 4 has the second marker on line 3 → looks back to it.
        assert_eq!(map.lookup(4).unwrap().jux_line, 2);
        // Line 5 also resolves to the same marker.
        assert_eq!(map.lookup(5).unwrap().jux_line, 2);
        // Line 1 has the first marker.
        assert_eq!(map.lookup(1).unwrap().jux_line, 1);
        // Line 0 (synthetic) returns None.
        assert!(map.lookup(0).is_none());
    }

    /// Malformed markers are skipped silently rather than poisoning
    /// the map.
    #[test]
    fn malformed_markers_are_dropped() {
        let src = "// JUX:not-a-real-marker\n// JUX:ok.jux:1:1";
        let map = MarkerMap::from_emitted_source(src);
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0].jux_path, "ok.jux");
    }

    /// The rewriter rewrites `--> src/main.rs:N:M` into the original
    /// `.jux` location plus an emitted-Rust parenthetical.
    #[test]
    fn rewrite_rewrites_arrow_lines() {
        let rust = "// JUX:test.jux:5:9\nfn main() {\n    \"oops\"\n}";
        let map = MarkerMap::from_emitted_source(rust);
        let stderr = "error[E0308]: mismatched types\n  --> src/main.rs:3:5\n   |";
        let rewritten = rewrite_rustc_output(stderr, &map);
        assert!(
            rewritten.contains("--> test.jux:5:9"),
            "missing jux anchor: {rewritten}",
        );
        assert!(
            rewritten.contains("(= src/main.rs:3:5)"),
            "missing emitted-rust parenthetical: {rewritten}",
        );
    }

    /// Lines without `--> ` pass through untouched.
    #[test]
    fn rewrite_leaves_other_lines_alone() {
        let map = MarkerMap::from_emitted_source("// JUX:a.jux:1:1\n");
        let stderr = "error[E0308]: mismatched types\n   |\n3 |     foo\n   |     ^^^\n";
        let rewritten = rewrite_rustc_output(stderr, &map);
        // Everything except `--> ...` should be byte-identical.
        assert!(rewritten.contains("error[E0308]: mismatched types"));
        assert!(rewritten.contains("|     foo"));
        assert!(rewritten.contains("|     ^^^"));
    }

    /// Empty map → stderr passes through verbatim. Used when the
    /// emitted crate was built without markers (`lower_with_types`).
    #[test]
    fn empty_map_passes_stderr_through() {
        let map = MarkerMap::default();
        let stderr = "error: something --> src/main.rs:5:1\n";
        assert_eq!(rewrite_rustc_output(stderr, &map), stderr);
    }

    /// Arrow lines whose emitted line has no covering marker pass
    /// through unchanged (still better than mis-attributing).
    #[test]
    fn rewrite_skips_unmapped_arrows() {
        // Marker on line 5 of the emitted Rust; rustc error on
        // line 2 has no preceding marker.
        let rust = "fn main() {\n}\n// JUX:x.jux:1:1\n";
        let map = MarkerMap::from_emitted_source(rust);
        let stderr = "  --> src/main.rs:2:1\n";
        let rewritten = rewrite_rustc_output(stderr, &map);
        // No marker covers line 2 → pass through verbatim.
        assert!(rewritten.contains("--> src/main.rs:2:1"));
        assert!(!rewritten.contains("(= "));
    }
}
