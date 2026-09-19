//! Rendering diagnostics for a terminal or a tool (JUX-DIAGNOSTICS-ADDENDUM
//! §D.1, §D.2). `juxc` and `jux` both print through here, so the two tools
//! show the same diagnostic the same way.
//!
//! Formats:
//!
//! - **`human`** (§D.1.3): the Rust-style block. A header
//!   `error[E0450]: message`, a `-->` location, the source line with the span
//!   underlined by carets, each label underlined with dashes, then `note:` and
//!   `help:` lines. Colored when the output is a terminal.
//! - **`compact`** (§D.1.4): `file:line:col: error[E0450]: message`, then one
//!   such line per label.
//! - **`short`** (§D.1.5): the primary line only.
//! - **`line`**: `file:line:col: [E0450] error: message`. This is what `juxc`
//!   printed before the other formats existed, and it stays the default when
//!   the output is not a terminal (ERRATA E72): the blessed UI outputs, the
//!   IDE's run console and people's scripts read it.
//! - **`json`** (§D.2): NDJSON, one object per diagnostic and a trailing
//!   summary line.

use std::fmt::Write as _;
use std::path::Path;

use juxc_diagnostics::{Diagnostic, Severity};
use juxc_source::{SourceFile, Span};

/// How diagnostics are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticFormat {
    /// §D.1.3, the multi-line block with source frames.
    Human,
    /// §D.1.4, one line per diagnostic and per label.
    Compact,
    /// §D.1.5, one line per diagnostic.
    Short,
    /// The historical one-line form (see the module docs).
    Line,
    /// §D.2, NDJSON.
    Json,
}

impl DiagnosticFormat {
    /// Parse a `--diagnostic-format` value.
    pub fn parse(s: &str) -> Option<DiagnosticFormat> {
        match s {
            "human" => Some(DiagnosticFormat::Human),
            "compact" => Some(DiagnosticFormat::Compact),
            "short" => Some(DiagnosticFormat::Short),
            "line" => Some(DiagnosticFormat::Line),
            "json" => Some(DiagnosticFormat::Json),
            _ => None,
        }
    }

    /// The format used when none was asked for: `human` for a person at a
    /// terminal, `line` for everything else (ERRATA E72).
    pub fn default_for(stderr_is_terminal: bool) -> DiagnosticFormat {
        if stderr_is_terminal {
            DiagnosticFormat::Human
        } else {
            DiagnosticFormat::Line
        }
    }
}

/// `--color auto|always|never`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    /// Color when writing to a terminal, `NO_COLOR` is unset and `TERM` is
    /// not `dumb`.
    Auto,
    /// Always color.
    Always,
    /// Never color.
    Never,
}

impl ColorChoice {
    /// Parse a `--color` value.
    pub fn parse(s: &str) -> Option<ColorChoice> {
        match s {
            "auto" => Some(ColorChoice::Auto),
            "always" => Some(ColorChoice::Always),
            "never" => Some(ColorChoice::Never),
            _ => None,
        }
    }

    /// Whether to emit ANSI colors on a stream that is (or is not) a terminal.
    pub fn enabled(self, is_terminal: bool) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                is_terminal
                    && std::env::var_os("NO_COLOR").is_none_or_empty()
                    && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
            }
        }
    }
}

/// `Option<OsString>::is_none_or_empty`, spelled for the MSRV.
trait NoneOrEmpty {
    fn is_none_or_empty(&self) -> bool;
}

impl NoneOrEmpty for Option<std::ffi::OsString> {
    fn is_none_or_empty(&self) -> bool {
        match self {
            None => true,
            Some(v) => v.is_empty(),
        }
    }
}

/// ANSI styles, all empty when color is off.
struct Palette {
    error: &'static str,
    warning: &'static str,
    note: &'static str,
    help: &'static str,
    bold: &'static str,
    blue: &'static str,
    reset: &'static str,
}

impl Palette {
    fn new(color: bool) -> Palette {
        if color {
            Palette {
                error: "\x1b[1;31m",
                warning: "\x1b[1;33m",
                note: "\x1b[1;34m",
                help: "\x1b[1;32m",
                bold: "\x1b[1m",
                blue: "\x1b[1;34m",
                reset: "\x1b[0m",
            }
        } else {
            Palette { error: "", warning: "", note: "", help: "", bold: "", blue: "", reset: "" }
        }
    }

    fn severity(&self, s: Severity) -> &'static str {
        match s {
            Severity::Error => self.error,
            Severity::Warning => self.warning,
            Severity::Note => self.note,
            Severity::Help => self.help,
        }
    }
}

/// The word printed for a severity.
pub fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
        Severity::Help => "help",
    }
}

/// Render `diagnostics` in a text format (everything but `json`), in source
/// order and de-duplicated, as one string ready for stderr.
pub fn render_text(
    diagnostics: &[Diagnostic],
    sources: &[SourceFile],
    format: DiagnosticFormat,
    color: bool,
) -> String {
    let p = Palette::new(color);
    let mut out = String::new();
    let ordered = crate::diagnostic_order::in_source_order(diagnostics);
    for d in &ordered {
        match format {
            DiagnosticFormat::Human => render_human(&mut out, d, sources, &p),
            DiagnosticFormat::Compact => render_compact(&mut out, d, sources, true),
            DiagnosticFormat::Short => render_compact(&mut out, d, sources, false),
            DiagnosticFormat::Line | DiagnosticFormat::Json => render_line(&mut out, d, sources),
        }
    }
    if format == DiagnosticFormat::Human {
        render_summary(&mut out, &ordered, &p);
    }
    out
}

/// The source file a diagnostic's primary span is in.
fn primary_source<'a>(d: &Diagnostic, sources: &'a [SourceFile]) -> Option<&'a SourceFile> {
    d.file.and_then(|i| sources.get(i))
}

/// The source a label's span is in: its own file when it names one, else the
/// diagnostic's file.
fn label_source<'a>(span: Span, d: &Diagnostic, sources: &'a [SourceFile]) -> Option<&'a SourceFile> {
    sources.get(span.file as usize).filter(|_| (span.file as usize) < sources.len()).or_else(|| primary_source(d, sources))
}

fn location(src: &SourceFile, span: Span) -> String {
    let (line, col) = char_line_col(src, span.start as usize);
    format!("{}:{line}:{col}", src.path().display())
}

/// The historical line format.
fn render_line(out: &mut String, d: &Diagnostic, sources: &[SourceFile]) {
    match (primary_source(d, sources), d.primary_span) {
        (Some(src), Some(span)) => {
            let (line, col) = src.line_col(span.start as usize);
            let _ = writeln!(
                out,
                "{}:{line}:{col}: [{}] {}: {}",
                src.path().display(),
                d.code,
                severity_label(d.severity),
                d.message,
            );
        }
        _ => {
            let _ = writeln!(out, "[{}] {}: {}", d.code, severity_label(d.severity), d.message);
        }
    }
}

/// `compact` (with labels) and `short` (without).
fn render_compact(out: &mut String, d: &Diagnostic, sources: &[SourceFile], with_labels: bool) {
    let head = format!("{}[{}]: {}", severity_label(d.severity), d.code, d.message);
    match (primary_source(d, sources), d.primary_span) {
        (Some(src), Some(span)) => {
            let _ = writeln!(out, "{}: {head}", location(src, span));
        }
        _ => {
            let _ = writeln!(out, "{head}");
        }
    }
    if with_labels {
        for l in &d.labels {
            match label_source(l.span, d, sources) {
                Some(src) => {
                    let _ = writeln!(out, "{}: note: {}", location(src, l.span), l.message);
                }
                None => {
                    let _ = writeln!(out, "note: {}", l.message);
                }
            }
        }
    }
}

/// §D.1.3.
fn render_human(out: &mut String, d: &Diagnostic, sources: &[SourceFile], p: &Palette) {
    let sev = p.severity(d.severity);
    let _ = writeln!(
        out,
        "{sev}{}[{}]{}{}: {}{}",
        severity_label(d.severity),
        d.code,
        p.reset,
        p.bold,
        d.message,
        p.reset,
    );
    // Every frame of this diagnostic shares one gutter width, sized for the
    // largest line number it prints.
    let mut widest = 1usize;
    let primary = primary_source(d, sources).zip(d.primary_span);
    if let Some((src, span)) = primary {
        widest = widest.max(digits(char_line_col(src, span.end.max(span.start) as usize).0));
    }
    for l in &d.labels {
        if let Some(src) = label_source(l.span, d, sources) {
            widest = widest.max(digits(char_line_col(src, l.span.end as usize).0));
        }
    }
    if let Some((src, span)) = primary {
        let _ = writeln!(out, "{}{}-->{} {}", " ".repeat(widest), p.blue, p.reset, location(src, span));
        frame(out, src, span, '^', "", sev, widest, p);
    }
    for l in &d.labels {
        match label_source(l.span, d, sources) {
            Some(src) if primary.is_some_and(|(ps, _)| std::ptr::eq(ps, src)) => {
                // Same file as the primary span: a labelled frame of its own.
                frame(out, src, l.span, '-', &l.message, p.blue, widest, p);
            }
            Some(src) => {
                let _ = writeln!(out, "{}note{}: {}", p.note, p.reset, l.message);
                let _ = writeln!(out, "{}{}-->{} {}", " ".repeat(widest), p.blue, p.reset, location(src, l.span));
                frame(out, src, l.span, '-', "", p.blue, widest, p);
            }
            None => {
                let _ = writeln!(out, "{}note{}: {}", p.note, p.reset, l.message);
            }
        }
    }
    for n in &d.notes {
        let _ = writeln!(out, "{}{} ={} {}note{}: {}", " ".repeat(widest), p.blue, p.reset, p.bold, p.reset, n);
    }
    for h in &d.help {
        let _ = writeln!(out, "{}{} ={} {}help{}: {}", " ".repeat(widest), p.blue, p.reset, p.help, p.reset, h);
    }
    out.push('\n');
}

/// One source frame: an empty gutter line, each line the span touches with
/// its number, and the underline (`^` for the primary span, `-` for a label)
/// with an optional caption after it.
#[allow(clippy::too_many_arguments)]
fn frame(
    out: &mut String,
    src: &SourceFile,
    span: Span,
    mark: char,
    caption: &str,
    mark_style: &str,
    width: usize,
    p: &Palette,
) {
    let text = src.contents();
    let start = (span.start as usize).min(text.len());
    let end = (span.end as usize).max(start).min(text.len());
    let (first, _) = char_line_col(src, start);
    let (last, _) = char_line_col(src, end.saturating_sub(usize::from(end > start)));
    let gutter = |out: &mut String, n: Option<u32>| {
        let label = n.map(|n| n.to_string()).unwrap_or_default();
        let _ = write!(out, "{}{label:>width$} |{}", p.blue, p.reset);
    };
    gutter(out, None);
    out.push('\n');
    // Long multi-line spans show their first and last line with `...` between.
    let lines: Vec<u32> = if last - first <= 3 {
        (first..=last).collect()
    } else {
        vec![first, first + 1, last]
    };
    let mut prev: Option<u32> = None;
    for n in lines {
        if prev.is_some_and(|p| n > p + 1) {
            let _ = writeln!(out, "{}...{}", p.blue, p.reset);
        }
        prev = Some(n);
        let Some((line_start, line_text)) = line_text(text, n) else {
            continue;
        };
        gutter(out, Some(n));
        let _ = writeln!(out, " {}", line_text.trim_end_matches('\r'));
        // The part of the span on this line, in character columns.
        let line_end = line_start + line_text.len();
        let from = start.max(line_start).min(line_end);
        let to = end.min(line_end).max(from);
        let pad = text[line_start..from].chars().map(|c| if c == '\t' { '\t' } else { ' ' }).collect::<String>();
        let len = text[from..to].chars().count().max(1);
        gutter(out, None);
        let underline: String = std::iter::repeat(mark).take(len).collect();
        if n == last && !caption.is_empty() {
            let _ = writeln!(out, " {pad}{mark_style}{underline} {caption}{}", p.reset);
        } else {
            let _ = writeln!(out, " {pad}{mark_style}{underline}{}", p.reset);
        }
    }
}

/// `(byte offset of the line's start, the line's text)` for 1-based line `n`.
fn line_text(text: &str, n: u32) -> Option<(usize, &str)> {
    let mut start = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        if i as u32 + 1 == n {
            return Some((start, line));
        }
        start += line.len() + 1;
    }
    None
}

/// 1-based line and 1-based character column (§D.2.2) of a byte offset.
fn char_line_col(src: &SourceFile, offset: usize) -> (u32, u32) {
    let text = src.contents();
    let offset = offset.min(text.len());
    let (line, byte_col) = src.line_col(offset);
    let line_start = offset + 1 - byte_col as usize;
    let col = text.get(line_start..offset).map(|s| s.chars().count()).unwrap_or(0) + 1;
    (line, col as u32)
}

fn digits(n: u32) -> usize {
    n.to_string().len()
}

/// The closing line of the human format, as Rust prints it.
fn render_summary(out: &mut String, ordered: &[&Diagnostic], p: &Palette) {
    let errors = ordered.iter().filter(|d| d.severity == Severity::Error).count();
    let warnings = ordered.iter().filter(|d| d.severity == Severity::Warning).count();
    let first_error = ordered.iter().find(|d| d.severity == Severity::Error);
    if let Some(d) = first_error {
        let _ = writeln!(
            out,
            "For more information about this error, try `juxc explain {}`.",
            d.code,
        );
    }
    let plural = |n: usize, word: &str| if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") };
    match (errors, warnings) {
        (0, 0) => {}
        (0, w) => {
            let _ = writeln!(out, "{}warning{}: {} emitted", p.warning, p.reset, plural(w, "warning"));
        }
        (e, 0) => {
            let _ = writeln!(out, "{}error{}: aborting due to {}", p.error, p.reset, plural(e, "error"));
        }
        (e, w) => {
            let _ = writeln!(
                out,
                "{}error{}: aborting due to {}; {} emitted",
                p.error,
                p.reset,
                plural(e, "error"),
                plural(w, "warning"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// JSON (§D.2)
// ---------------------------------------------------------------------------

/// Diagnostics as NDJSON (§D.2): one object per diagnostic in source order,
/// then a `{"summary":…}` line (§D.2.4). `duration_ms` is what the caller
/// measured around the compile, or 0.
pub fn render_json(diagnostics: &[Diagnostic], sources: &[SourceFile], duration_ms: u128) -> String {
    let mut out = String::new();
    let mut errors = 0u32;
    let mut warnings = 0u32;
    for d in crate::diagnostic_order::in_source_order(diagnostics) {
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            _ => {}
        }
        let mut fields: Vec<String> = Vec::new();
        fields.push(format!("\"code\":{}", json_str(&d.code.to_string())));
        fields.push(format!("\"severity\":{}", json_str(severity_label(d.severity))));
        fields.push(format!("\"message\":{}", json_str(&d.message)));
        if let (Some(src), Some(span)) = (primary_source(d, sources), d.primary_span) {
            fields.push(format!("\"primary_span\":{}", span_json(src, span, None, None)));
            if !d.labels.is_empty() {
                let arr: Vec<String> = d
                    .labels
                    .iter()
                    .filter_map(|l| {
                        label_source(l.span, d, sources).map(|s| span_json(s, l.span, Some(&l.message), Some("note")))
                    })
                    .collect();
                fields.push(format!("\"secondary_spans\":[{}]", arr.join(",")));
            }
        }
        if let Some(hint) = d.help.first() {
            fields.push(format!("\"hint\":{}", json_str(hint)));
        }
        fields.push(format!(
            "\"docs_url\":{}",
            json_str(&format!("https://docs.jux-lang.org/diag/{}", d.code)),
        ));
        let _ = writeln!(out, "{{{}}}", fields.join(","));
    }
    let _ = writeln!(
        out,
        "{{\"summary\":{{\"errors\":{errors},\"warnings\":{warnings},\"files_compiled\":{},\"duration_ms\":{duration_ms}}}}}",
        sources.len(),
    );
    out
}

/// One span as a §D.2.2 object. `label` and `severity` are set for secondary
/// spans. Columns are 1-based character columns; `highlight_*` are 0-based
/// byte offsets into the snippet line.
fn span_json(src: &SourceFile, span: Span, label: Option<&str>, severity: Option<&str>) -> String {
    let text = src.contents();
    let start = (span.start as usize).min(text.len());
    let end = (span.end as usize).min(text.len());
    let (line_start, start_line_byte) = line_and_start(src, start);
    let (line_end, end_line_byte) = line_and_start(src, end);
    let snippet_end = text[start_line_byte..].find('\n').map(|n| start_line_byte + n).unwrap_or(text.len());
    let snippet = &text[start_line_byte..snippet_end];
    let column_start = text[start_line_byte..start].chars().count() + 1;
    let column_end = text[end_line_byte..end].chars().count() + 1;
    let highlight_start = start - start_line_byte;
    let highlight_end = (end - start_line_byte).min(snippet.len());
    let mut f: Vec<String> = Vec::new();
    f.push(format!("\"file\":{}", json_str(&fwd_slash(src.path()))));
    f.push(format!("\"byte_start\":{start}"));
    f.push(format!("\"byte_end\":{end}"));
    f.push(format!("\"line_start\":{line_start}"));
    f.push(format!("\"line_end\":{line_end}"));
    f.push(format!("\"column_start\":{column_start}"));
    f.push(format!("\"column_end\":{column_end}"));
    f.push(format!("\"snippet\":{}", json_str(snippet)));
    f.push(format!("\"highlight_start\":{highlight_start}"));
    f.push(format!("\"highlight_end\":{highlight_end}"));
    if let Some(l) = label {
        f.push(format!("\"label\":{}", json_str(l)));
    }
    if let Some(s) = severity {
        f.push(format!("\"severity\":{}", json_str(s)));
    }
    format!("{{{}}}", f.join(","))
}

/// `(1-based line, byte offset of that line's start)` for `offset`.
fn line_and_start(src: &SourceFile, offset: usize) -> (u32, usize) {
    let (line, byte_col) = src.line_col(offset);
    (line, offset + 1 - byte_col as usize)
}

fn fwd_slash(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// A JSON string literal with the mandatory escapes.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use juxc_diagnostics::code::Code;

    fn sample() -> (Vec<Diagnostic>, Vec<SourceFile>) {
        let text = "public void main() {\n    int x = \"no\";\n}\n";
        let src = SourceFile::new(std::path::PathBuf::from("src/main.jux"), text.to_string());
        let start = text.find("\"no\"").unwrap() as u32;
        let mut d = Diagnostic::error(Code::E0410_TypeMismatch, "type mismatch: expected int, found String")
            .with_span(Span { start, end: start + 4, file: 0 });
        d.file = Some(0);
        d.help.push("convert with `.parse<int>()`".to_string());
        (vec![d], vec![src])
    }

    #[test]
    fn human_frames_underline_the_span() {
        let (d, s) = sample();
        let text = render_text(&d, &s, DiagnosticFormat::Human, false);
        assert!(text.starts_with("error[E0410]: type mismatch: expected int, found String\n"), "{text}");
        assert!(text.contains(" --> src/main.jux:2:13\n"), "{text}");
        assert!(text.contains("2 |     int x = \"no\";\n"), "{text}");
        assert!(text.contains("  |             ^^^^\n"), "{text}");
        assert!(text.contains(" = help: convert with `.parse<int>()`"), "{text}");
        assert!(text.contains("error: aborting due to 1 error"), "{text}");
        assert!(!text.contains('\x1b'), "no color was asked for");
        let colored = render_text(&d, &s, DiagnosticFormat::Human, true);
        assert!(colored.contains("\x1b[1;31merror[E0410]"), "{colored}");
    }

    #[test]
    fn one_line_formats() {
        let (d, s) = sample();
        assert_eq!(
            render_text(&d, &s, DiagnosticFormat::Short, false),
            "src/main.jux:2:13: error[E0410]: type mismatch: expected int, found String\n",
        );
        assert_eq!(
            render_text(&d, &s, DiagnosticFormat::Line, false),
            "src/main.jux:2:13: [E0410] error: type mismatch: expected int, found String\n",
        );
        let json = render_json(&d, &s, 5);
        assert!(json.contains("\"code\":\"E0410\"") && json.contains("\"duration_ms\":5"), "{json}");
    }
}
