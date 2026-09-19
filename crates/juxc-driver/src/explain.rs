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
            out.push_str(&format!("{code}: {desc}\n"));
            if source.chars().any(|c| c.is_alphanumeric()) {
                out.push_str(&format!("Defined in: {source}\n"));
            }
        }
        None => out.push_str(&format!("{code}\n")),
    }
    if let Some(doc) = doc {
        out.push('\n');
        out.push_str(&doc);
        out.push('\n');
    }
    for s in sections {
        out.push('\n');
        out.push_str(&s);
        out.push('\n');
    }
    Some(out)
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
            return Some(reflow(&text));
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
}
