//! `juxc explain <CODE>` (JUX-DIAGNOSTICS-ADDENDUM §D.5.3): the documentation
//! of one diagnostic code, printed in the terminal, offline.
//!
//! "The doc text is bundled with the compiler": both sources are embedded at
//! build time, so the text always matches the compiler that prints it.
//!
//! - The code's entry in the diagnostics catalog (§D.4): its one-line
//!   description and the spec section that defines it.
//! - The doc comment on the code's variant in `juxc-diagnostics`
//!   (`/// E0414 — Access to a private member ...`), which says what the check
//!   is and when it fires.
//! - Any section of the diagnostics addendum written about the code (its
//!   heading names the code, as in `### Java Habits (E0413, E0200, E0301)`).

/// The diagnostics addendum, embedded.
const ADDENDUM: &str = include_str!("../../../Architecture/JUX-DIAGNOSTICS-ADDENDUM.md");

/// The code table, embedded for its per-code doc comments.
const CODES: &str = include_str!("../../juxc-diagnostics/src/code.rs");

/// Normalize user input: `e0413`, `0413`, `E413` all mean `E0413`; warnings
/// keep their `W`.
pub fn normalize(code: &str) -> String {
    let t = code.trim().to_ascii_uppercase();
    let (letter, digits) = match t.chars().next() {
        Some(c @ ('E' | 'W' | 'L' | 'N')) => (c, t[1..].to_string()),
        _ => ('E', t.clone()),
    };
    if digits.chars().all(|c| c.is_ascii_digit()) && !digits.is_empty() && digits.len() <= 4 {
        format!("{letter}{digits:0>4}")
    } else {
        t
    }
}

/// The explanation for `code`, or `None` when no source mentions it.
///
/// What is printed is for the person who got the diagnostic: what the code
/// means and what to do about it, in Jux terms. The sources it is drawn from
/// are written for the compiler's own authors as well, and they also say how
/// a construct is lowered to the code Jux compiles to, what the build tools
/// reported before a check existed, and which ERRATA entry settled the rule.
/// Every paragraph (and every sentence of the one-line description) that
/// talks about that is left out ([`for_the_user`]); the specification keeps
/// its rationale, and `Defined in` points at it (gap 35).
pub fn explain(code: &str) -> Option<String> {
    let code = normalize(code);
    let catalog = catalog_row(&code);
    let doc = variant_doc(&code);
    let sections = addendum_sections(&code);
    if catalog.is_none() && doc.is_none() && sections.is_empty() {
        return None;
    }
    let mut out = String::new();
    match &catalog {
        Some((desc, source)) => {
            let desc = override_description(&code)
                .map(str::to_string)
                .unwrap_or_else(|| user_sentences(desc));
            out.push_str(&format!("{code}: {desc}\n"));
            // The specification section, without the ERRATA entry that
            // records how the rule was settled.
            let source = source
                .split(" / ")
                .filter(|part| !part.contains("ERRATA"))
                .collect::<Vec<_>>()
                .join(" / ");
            if source.chars().any(|c| c.is_alphanumeric()) {
                out.push_str(&format!("Defined in: {source}\n"));
            }
        }
        None => out.push_str(&format!("{code}\n")),
    }
    if let Some(doc) = doc.filter(|d| !d.trim().is_empty()) {
        out.push('\n');
        out.push_str(&doc);
        out.push('\n');
    }
    for s in sections {
        let s = user_paragraphs(&s);
        // A heading with nothing left under it says nothing.
        if s.lines().filter(|l| !l.trim().is_empty()).count() < 2 {
            continue;
        }
        out.push('\n');
        out.push_str(&s);
        out.push('\n');
    }
    Some(out)
}

/// The one-line description of a code whose catalog row is written about the
/// compiler's internals rather than about the program.
fn override_description(code: &str) -> Option<&'static str> {
    Some(match code {
        "E0900" => {
            "internal compiler error: the compiler produced a program it could not build. This \
             is a bug in the Jux compiler, not in your program; please report it with the source \
             that triggered it"
        }
        "E0904" => "`--target` names a target whose standard library is not installed for the toolchain Jux builds with",
        "E0305" => "a name that the code Jux compiles to cannot spell (`self`, `Self`, `crate`, `super`); rename it",
        _ => return None,
    })
}

/// Whether a piece of the explanation is about the compiler rather than the
/// program: how it is lowered, what the generated code or the build tools
/// did, or the ERRATA history behind the rule.
fn about_the_compiler(text: &str) -> bool {
    if juxc_diagnostics::leak::find_rust_leak(text).is_some() {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    // "Rust" alone is not on the list: Rust's standard library is Jux's
    // (`rust.std.Vec`), and saying so is about the program.
    const TERMS: &[&str] = &[
        "rustc", "rustdoc", "rustfmt", "rustup", "cargo", "backend", "lowering", "lowered",
        "lowers ", "lower to", "lower it", "emitted", "emitter", "the generated", "generated rust",
        "generated crate", "rust crate", "rust code", "rust compiler", "`rc<", "`rc`", "refcell",
        "trait object", "errata",
        "phase 1", "phase-1", "pre-fix", "codegen", "monomorph",
    ];
    TERMS.iter().any(|t| lower.contains(t))
}

/// `text` without the paragraphs (and list items) [`about_the_compiler`].
fn user_paragraphs(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let flush = |current: &mut Vec<&str>, out: &mut Vec<String>| {
        if current.is_empty() {
            return;
        }
        let para = current.join("\n");
        if !about_the_compiler(&para) {
            out.push(para);
        }
        current.clear();
    };
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            in_fence = !in_fence;
            current.push(line);
            if !in_fence {
                flush(&mut current, &mut out);
            }
            continue;
        }
        if in_fence {
            current.push(line);
            continue;
        }
        if t.is_empty() {
            flush(&mut current, &mut out);
            continue;
        }
        if t.starts_with("- ") || t.starts_with("* ") || t.starts_with('|') {
            flush(&mut current, &mut out);
        }
        current.push(line);
    }
    flush(&mut current, &mut out);
    out.join("\n\n")
}

/// The sentences of a one-line description that are about the program.
fn user_sentences(desc: &str) -> String {
    let kept: Vec<&str> = desc
        .split_inclusive(". ")
        .filter(|s| !about_the_compiler(s))
        .collect();
    let text = kept.concat();
    let text = text.trim().trim_end_matches('.').trim();
    if text.is_empty() {
        "see the specification section below".to_string()
    } else {
        text.to_string()
    }
}

/// `(description, source)` from the catalog table row for `code`.
fn catalog_row(code: &str) -> Option<(String, String)> {
    let needle = format!("`{code}`");
    for line in ADDENDUM.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
        if cells.first().is_some_and(|c| *c == needle || c.starts_with(&needle)) {
            let desc = cells.get(1).copied().unwrap_or("").to_string();
            let source = cells.get(2).copied().unwrap_or("").to_string();
            return Some((desc, source));
        }
    }
    None
}

/// Whether this compiler has a `code` it can raise: a variant named
/// `<code>_...` in the code table. A code the catalog only reserves is not one,
/// so a lint level for it could never take effect (§D.5.4).
pub(crate) fn is_known_code(code: &str) -> bool {
    let prefix = format!("{code}_");
    CODES.lines().any(|line| {
        let t = line.trim();
        t.starts_with(&prefix) && t.ends_with(',') && !t.contains("=>")
    })
}

/// The doc comment of the enum variant for `code` in `code.rs`: the `///`
/// lines right above a variant named `<code>_...`, with the leading
/// `E0414 — ` taken off the first line.
fn variant_doc(code: &str) -> Option<String> {
    let prefix = format!("{code}_");
    let mut doc: Vec<&str> = Vec::new();
    for line in CODES.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("///") {
            doc.push(rest.strip_prefix(' ').unwrap_or(rest));
            continue;
        }
        if t.starts_with(&prefix) && t.ends_with(',') && !t.contains("=>") {
            if doc.is_empty() {
                return None;
            }
            let mut text = doc.join("\n");
            // Drop the `E0414 — ` / `E0414: ` lead-in the comment repeats.
            for sep in [" — ", " -- ", ": ", " - "] {
                if let Some(rest) = text.strip_prefix(&format!("{code}{sep}")) {
                    text = rest.to_string();
                    break;
                }
            }
            // Paragraph boundaries are the blank `///` lines, which
            // `reflow` folds away, so the filter runs first.
            return Some(reflow(&user_paragraphs(&text)));
        }
        doc.clear();
    }
    None
}

/// Join a doc comment's wrapped lines into paragraphs, keeping blank lines
/// and list items as breaks, and re-wrap to 78 columns for a terminal.
fn reflow(text: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let t = line.trim();
        let starts_item = t.starts_with("- ") || t.starts_with("* ");
        if t.is_empty() || starts_item {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            if t.is_empty() {
                continue;
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(t);
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs.iter().map(|p| wrap(p, 78)).collect::<Vec<_>>().join("\n")
}

fn wrap(text: &str, width: usize) -> String {
    let indent = if text.starts_with("- ") || text.starts_with("* ") { "  " } else { "" };
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push_str(&line);
            out.push('\n');
            line = indent.to_string();
        }
        if !line.is_empty() && line != indent {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push_str(&line);
    out
}

/// Sections of the addendum whose heading names `code`, with their text up
/// to the next heading of the same or a higher level.
fn addendum_sections(code: &str) -> Vec<String> {
    let lines: Vec<&str> = ADDENDUM.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let level = line.chars().take_while(|c| *c == '#').count();
        if level >= 2 && line.contains(code) && !line.contains("–") {
            let mut text = vec![line.trim_start_matches('#').trim().to_string(), String::new()];
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                let lv = l.chars().take_while(|c| *c == '#').count();
                if lv > 0 && lv <= level {
                    break;
                }
                text.push(l.to_string());
                j += 1;
            }
            while text.last().is_some_and(|l| l.trim().is_empty() || l.trim() == "---") {
                text.pop();
            }
            out.push(text.join("\n"));
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_normalize() {
        assert_eq!(normalize("e413"), "E0413");
        assert_eq!(normalize("0301"), "E0301");
        assert_eq!(normalize("W0457"), "W0457");
    }

    #[test]
    fn a_known_code_explains_itself() {
        let text = explain("E0414").expect("E0414 is documented");
        assert!(text.starts_with("E0414: "), "{text}");
        assert!(text.to_lowercase().contains("private"), "{text}");
    }

    #[test]
    fn an_unknown_code_is_none() {
        assert!(explain("E9876").is_none());
    }

    /// Every code this compiler can raise explains itself in Jux terms: no
    /// Rust leak, and nothing about how the construct is lowered or what the
    /// build tools said (gap 35).
    #[test]
    fn every_explanation_is_about_the_program() {
        let codes: Vec<String> = CODES
            .lines()
            .filter_map(|l| l.split_once("=> \"").and_then(|(_, r)| r.split_once('"')).map(|(c, _)| c.to_string()))
            .filter(|c| c.len() == 5 && (c.starts_with('E') || c.starts_with('W')))
            .collect();
        assert!(codes.len() > 100, "the code scan found {}", codes.len());
        let mut bad = Vec::new();
        for code in &codes {
            let Some(text) = explain(code) else { continue };
            for para in text.split("\n\n") {
                if about_the_compiler(para) {
                    bad.push(format!("{code}: {para}"));
                }
            }
        }
        assert!(bad.is_empty(), "{} explanation paragraph(s) talk about the compiler:\n{}", bad.len(), bad.join("\n---\n"));
    }

    #[test]
    fn the_internal_error_explains_itself_in_jux_terms() {
        let text = explain("E0900").expect("E0900 is documented");
        assert!(text.contains("bug in the Jux compiler"), "{text}");
        assert!(!text.contains("rustc"), "{text}");
    }
}
