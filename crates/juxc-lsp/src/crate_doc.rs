//! A crate's doc comments in Jux terms (GAPS.md gap 38).
//!
//! A `rust.<crate>` stub carries the summary line of the crate's own
//! documentation, and that text is written for Rust programmers: intra-doc
//! links (``[`Ui::add`]``, ``[`Area`](crate::containers::area::Area)``),
//! Rust types (`Option<T>`, `&str`, `&mut self`, `Box<dyn Widget>`,
//! lifetimes), code blocks in Rust, and `# Safety` sections about `unsafe`.
//! Hover and completion show a stub's doc after [`jux_doc`] has rewritten it:
//!
//! - an intra-doc link is its target's Jux name, in code: ``[`crate::Slider`]``
//!   is `` `Slider` ``, ``[`Ui::add`]`` is `` `Ui.add` ``, and a link whose
//!   text is prose (`[viewport](crate::viewport)`, `[B-Tree]`) is its text;
//!   a link to a web page stays a link;
//! - `Option<T>` is `T?`; `&T`, `&mut T` and lifetimes are dropped, `&str` and
//!   `str` are `String`, `&self`/`&mut self` is `this`; `Box<T>`, `Rc<T>`,
//!   `Arc<T>`, `RefCell<T>`, `dyn Trait` and `impl Trait` are their inner
//!   type; a module path (`std::io::Error`, `epaint::PaintCallback`) is the
//!   type's own name, and `Type::member` is `Type.member`; inside code, a
//!   Rust number type is its Jux name (`u8` is `ubyte`, `usize` is `uint`);
//! - a code block is dropped unless it is marked `jux` or `text`, and so is a
//!   `# Safety` section; a heading left with nothing under it goes too.
//!
//! The rewrite runs when the doc is shown (one line, in practice), so a stub
//! generated before this rule, or written by hand, reads the same.

/// Whether `path` is a crate's generated (or hand-written) declaration stub,
/// a `.jux.d` file, rather than a Jux source.
pub(crate) fn is_crate_stub(path: &std::path::Path) -> bool {
    path.to_string_lossy().ends_with(".jux.d")
}

/// A stub's doc as an editor shows it: [`jux_doc`], unless what is left
/// still shows Rust that none of `sources` (the open buffer) contains, in
/// which case it is withheld (and, under the self-check, reported).
pub(crate) fn shown(doc: &str, sources: &[&str]) -> Option<String> {
    let text = jux_doc(doc);
    (!text.trim().is_empty() && !crate::leak_guard::shows_rust("crate documentation", &text, sources)).then_some(text)
}

/// `doc`, a crate stub's doc comment, in Jux terms.
pub(crate) fn jux_doc(doc: &str) -> String {
    let blocks = drop_blocks(doc);
    let linked = rewrite_links(&blocks);
    outside_urls(&linked, rewrite_types)
}

/// Drop code blocks not marked `jux` or `text`, `# Safety` sections, and
/// headings with nothing left under them.
fn drop_blocks(doc: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut fence: Option<bool> = None; // Some(keep) inside a fence
    let mut skip_level: Option<usize> = None;
    for line in doc.lines() {
        let t = line.trim();
        if let Some(keep) = fence {
            if t.starts_with("```") {
                fence = None;
            }
            if keep && skip_level.is_none() {
                kept.push(line.to_string());
            }
            continue;
        }
        if let Some(info) = t.strip_prefix("```") {
            let keep = matches!(info.trim(), "jux" | "text");
            fence = Some(keep);
            if keep && skip_level.is_none() {
                kept.push(line.to_string());
            }
            continue;
        }
        let level = t.chars().take_while(|c| *c == '#').count();
        if level > 0 && t[level..].starts_with(' ') {
            if skip_level.is_some_and(|l| level > l) {
                continue;
            }
            skip_level = None;
            if t[level..].trim().eq_ignore_ascii_case("safety") {
                skip_level = Some(level);
                continue;
            }
        }
        if skip_level.is_none() {
            kept.push(line.to_string());
        }
    }
    // A heading followed by nothing but blank lines up to the next heading
    // of its level or above (or the end) has lost its section.
    let heading = |s: &str| {
        let t = s.trim();
        let level = t.chars().take_while(|c| *c == '#').count();
        (level > 0 && t[level..].starts_with(' ')).then_some(level)
    };
    let mut out: Vec<String> = Vec::new();
    for (i, line) in kept.iter().enumerate() {
        if let Some(level) = heading(line) {
            let body = kept[i + 1..]
                .iter()
                .take_while(|l| heading(l).map_or(true, |h| h > level))
                .any(|l| !l.trim().is_empty() && heading(l).is_none());
            if !body {
                continue;
            }
        }
        if line.trim().is_empty() && out.last().map_or(true, |l: &String| l.trim().is_empty()) {
            continue;
        }
        out.push(line.clone());
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// Rewrite rustdoc links: ``[`X`]``, ``[`X`](target)``, `[text](target)`,
/// `[text][ref]` and a bare `[text]`.
fn rewrite_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = link_label_end(after) else {
            out.push('[');
            rest = after;
            continue;
        };
        let label = &after[..close];
        let mut tail = &after[close + 1..];
        let mut target: Option<&str> = None;
        if let Some(t) = tail.strip_prefix('(') {
            if let Some(end) = t.find(')') {
                target = Some(&t[..end]);
                tail = &t[end + 1..];
            }
        } else if let Some(t) = tail.strip_prefix('[') {
            if let Some(end) = t.find(']') {
                target = Some(&t[..end]);
                tail = &t[end + 1..];
            }
        }
        let web = target.is_some_and(|t| t.starts_with("http://") || t.starts_with("https://"));
        let code = label.len() >= 2 && label.starts_with('`') && label.ends_with('`');
        let prose = label.chars().next().is_some_and(char::is_alphabetic)
            && label.chars().all(|c| c.is_alphanumeric() || " -_.:'@".contains(c));
        if code {
            out.push('`');
            out.push_str(&jux_name(&label[1..label.len() - 1]));
            out.push('`');
        } else if web {
            out.push('[');
            out.push_str(label);
            out.push_str("](");
            out.push_str(target.unwrap_or_default());
            out.push(')');
        } else if target.is_some() || prose {
            out.push_str(&rewrite_paths(label));
        } else {
            // `[0, 1]` is not a link.
            out.push('[');
            out.push_str(label);
            out.push(']');
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Where the label of a link opened just before `s` ends: the `]` outside
/// backticks.
fn link_label_end(s: &str) -> Option<usize> {
    let mut in_code = false;
    for (i, c) in s.char_indices() {
        match c {
            '`' => in_code = !in_code,
            ']' if !in_code => return Some(i),
            '[' | '\n' if !in_code => return None,
            _ => {}
        }
    }
    None
}

/// An intra-doc link's code, `crate::Slider` or `Ui::add()` or `struct@Foo`,
/// as the Jux name of what it names.
fn jux_name(code: &str) -> String {
    let code = code.split_once('@').map_or(code, |(kind, rest)| {
        if kind.chars().all(|c| c.is_ascii_lowercase()) {
            rest
        } else {
            code
        }
    });
    let (path, suffix) = match code.strip_suffix("()") {
        Some(p) => (p, "()"),
        None => (code.strip_suffix('!').unwrap_or(code), ""),
    };
    let named = if path.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':') {
        jux_primitive(&rewrite_paths(path)).to_string()
    } else {
        rewrite_types(path)
    };
    format!("{named}{suffix}")
}

/// A Rust number or string type's Jux name (Bindgen §G.3.1).
fn jux_primitive(word: &str) -> &str {
    match word {
        "i8" => "byte",
        "i16" => "short",
        "i64" => "long",
        "isize" => "int",
        "u8" => "ubyte",
        "u16" => "ushort",
        "u64" => "ulong",
        "usize" => "uint",
        "f32" => "float",
        "f64" => "double",
        "str" => "String",
        other => other,
    }
}

/// Apply `f` to the text outside the targets of web links, which are kept
/// as written.
fn outside_urls(text: &str, f: fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("](http") {
        let end = rest[at..].find(')').map_or(rest.len(), |e| at + e + 1);
        out.push_str(&f(&rest[..at]));
        out.push_str(&rest[at..end]);
        rest = &rest[end..];
    }
    out.push_str(&f(rest));
    out
}

/// A module path is the name of what it names: `std::io::Error` is `Error`,
/// `Ui::add` is `Ui.add`, `crate::viewport` is `viewport`.
fn rewrite_paths(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let starts_word = (c.is_alphabetic() || c == '_') && (i == 0 || !is_ident(chars[i - 1]));
        if !starts_word {
            out.push(c);
            i += 1;
            continue;
        }
        let mut segments: Vec<String> = Vec::new();
        let mut current = String::new();
        while i < chars.len() {
            if is_ident(chars[i]) {
                current.push(chars[i]);
                i += 1;
            } else if chars[i] == ':' && chars.get(i + 1) == Some(&':') && chars.get(i + 2).is_some_and(|c| is_ident(*c)) {
                segments.push(std::mem::take(&mut current));
                i += 2;
            } else {
                break;
            }
        }
        segments.push(current);
        let type_at = segments.iter().rposition(|s| s.chars().next().is_some_and(|c| c.is_uppercase()));
        let shown = match type_at {
            Some(t) => segments[t..].join("."),
            None => segments.last().cloned().unwrap_or_default(),
        };
        out.push_str(&shown);
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Rust type syntax in Jux's: see the module documentation.
fn rewrite_types(text: &str) -> String {
    let mut s = rewrite_paths(text);
    s = drop_lifetimes(&s);
    s = rewrite_refs(&s);
    for wrapper in ["Box", "Rc", "Arc", "RefCell"] {
        s = unwrap_generic(&s, wrapper, &|inner| inner.to_string());
    }
    s = unwrap_generic(&s, "Option", &|inner| format!("{inner}?"));
    for kw in ["dyn ", "impl "] {
        s = drop_keyword(&s, kw);
    }
    in_code_spans(&s)
}

/// Inside backticks, a Rust number type is its Jux name and `self` is `this`.
fn in_code_spans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, part) in text.split('`').enumerate() {
        if i > 0 {
            out.push('`');
        }
        if i % 2 == 1 {
            let mut word = String::new();
            for c in part.chars().chain(std::iter::once('\0')) {
                if is_ident(c) {
                    word.push(c);
                    continue;
                }
                if !word.is_empty() {
                    out.push_str(if word == "self" { "this" } else { jux_primitive(&word) });
                    word.clear();
                }
                if c != '\0' {
                    out.push(c);
                }
            }
        } else {
            out.push_str(part);
        }
    }
    out
}

/// `'a`, `'static` and `'_` in type position (after `&` or `<`, or before
/// `,`, `>` or a space), with a separator left dangling; a char literal
/// (`'a'`) and an apostrophe in prose (`crate's`) stay.
fn drop_lifetimes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = out.chars().next_back();
        if c == '\'' && !prev.is_some_and(is_ident) {
            let mut j = i + 1;
            while j < chars.len() && is_ident(chars[j]) {
                j += 1;
            }
            let name: String = chars[i + 1..j].iter().collect();
            let next = chars.get(j).copied();
            let lifetime = !name.is_empty()
                && name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
                && next != Some('\'')
                && (matches!(prev, Some('&') | Some('<')) || matches!(next, Some(',') | Some('>')) || name == "static");
            if lifetime {
                i = j;
                // `<'a, T>` → `<T>`, `&'a T` → `&T`, `Foo<'a>` → `Foo`.
                while chars.get(i) == Some(&',') || chars.get(i) == Some(&' ') {
                    i += 1;
                }
                if prev == Some('<') && chars.get(i) == Some(&'>') {
                    out.pop();
                    i += 1;
                }
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `&mut self` and `&self` are `this`; `&mut T` and `&T` are `T`; `&str` is
/// `String`. An `&` standing alone in prose stays.
fn rewrite_refs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let mut after = &rest[at + 1..];
        if !after.chars().next().is_some_and(|c| is_ident(c) || c == '[' || c == '(') {
            out.push('&');
            rest = after;
            continue;
        }
        if let Some(m) = after.strip_prefix("mut ") {
            after = m;
        }
        let word: String = after.chars().take_while(|c| is_ident(*c)).collect();
        match word.as_str() {
            "self" => out.push_str("this"),
            "str" => out.push_str("String"),
            _ => out.push_str(&word),
        }
        rest = &after[word.len()..];
    }
    out.push_str(rest);
    out
}

/// `Name<inner>` as `f(inner)`, for every `Name<` at a word start, the
/// angle brackets balanced.
fn unwrap_generic(text: &str, name: &str, f: &dyn Fn(&str) -> String) -> String {
    let open = format!("{name}<");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&open) {
        let before = if at == 0 { out.chars().next_back() } else { rest[..at].chars().next_back() };
        let own_word = !before.is_some_and(is_ident);
        let body = &rest[at + open.len()..];
        let mut depth = 1;
        let mut end = None;
        for (i, c) in body.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end.filter(|_| own_word) {
            Some(end) => {
                out.push_str(&rest[..at]);
                let inner = unwrap_generic(&body[..end], name, f);
                out.push_str(&f(inner.trim()));
                rest = &body[end + 1..];
            }
            None => {
                out.push_str(&rest[..at + open.len()]);
                rest = body;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `dyn Trait` and `impl Trait` as `Trait`.
fn drop_keyword(text: &str, kw: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(kw) {
        let own_word = !rest[..at].chars().next_back().is_some_and(is_ident);
        let typed = rest[at + kw.len()..].chars().next().is_some_and(|c| c.is_uppercase());
        out.push_str(&rest[..at]);
        if !(own_word && typed) {
            out.push_str(kw);
        }
        rest = &rest[at + kw.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::jux_doc;

    #[test]
    fn intra_doc_links_are_jux_names() {
        assert_eq!(
            jux_doc("Anything implementing Widget can be added to a [`Ui`] with [`Ui::add`]."),
            "Anything implementing Widget can be added to a `Ui` with `Ui.add`."
        );
        assert_eq!(
            jux_doc("Describes a widget such as a [`crate::Button`] or a [`crate::TextEdit`]."),
            "Describes a widget such as a `Button` or a `TextEdit`."
        );
        assert_eq!(jux_doc("Args passed when sizing an [`super::Atom`]"), "Args passed when sizing an `Atom`");
        assert_eq!(
            jux_doc("A `BarrierWaitResult` is returned by [`Barrier::wait()`] when all threads"),
            "A `BarrierWaitResult` is returned by `Barrier.wait()` when all threads"
        );
        assert_eq!(
            jux_doc("An error returned by [`LocalKey::try_with`](struct.LocalKey.html#method.try_with)."),
            "An error returned by `LocalKey.try_with`."
        );
        assert_eq!(
            jux_doc("Keeps track of [`Area`](crate::containers::area::Area)s, which are free-floating [`Ui`](crate::Ui)s."),
            "Keeps track of `Area`s, which are free-floating `Ui`s."
        );
        assert_eq!(
            jux_doc("An input event from the backend into egui, about a specific [viewport](crate::viewport)."),
            "An input event from the backend into egui, about a specific viewport."
        );
        assert_eq!(jux_doc("An ordered map based on a [B-Tree]."), "An ordered map based on a B-Tree.");
        assert_eq!(jux_doc("Describes a [Buffer](crate::Buffer) when allocating."), "Describes a Buffer when allocating.");
        assert_eq!(
            jux_doc("Helper trait for all types that can be parsed as a [`font_types::Tag`]."),
            "Helper trait for all types that can be parsed as a `Tag`."
        );
    }

    #[test]
    fn web_links_and_brackets_that_are_not_links_stay() {
        assert_eq!(
            jux_doc("A 2D point. Derived from [kurbo](https://github.com/linebender/kurbo)."),
            "A 2D point. Derived from [kurbo](https://github.com/linebender/kurbo)."
        );
        assert_eq!(
            jux_doc("Defines how textures are wrapped outside the [0, 1] range."),
            "Defines how textures are wrapped outside the [0, 1] range."
        );
    }

    #[test]
    fn rust_types_are_written_the_jux_way() {
        assert_eq!(jux_doc("Returns an `Option<Rect>` if any."), "Returns an `Rect?` if any.");
        assert_eq!(jux_doc("A slice of a path (akin to [`str`])."), "A slice of a path (akin to `String`).");
        assert_eq!(jux_doc("An [`i64`] that is known not to equal zero."), "An `long` that is known not to equal zero.");
        assert_eq!(jux_doc("Takes `&mut self` and a `&'a str`."), "Takes `this` and a `String`.");
        assert_eq!(jux_doc("A wrapper around `dyn Any`, used for passing custom user data"), "A wrapper around `Any`, used for passing custom user data");
        assert_eq!(jux_doc("Holds a `Box<dyn Widget>` and an `Arc<Mutex<Vec<u8>>>`."), "Holds a `Widget` and an `Mutex<Vec<ubyte>>`.");
        assert_eq!(jux_doc("Reads into `Cow<'a, str>` from `std::io::Stdin`."), "Reads into `Cow<String>` from `Stdin`.");
        assert_eq!(jux_doc("Implementation of the `GetThreadId` trait for `lock_api::ReentrantMutex`."), "Implementation of the `GetThreadId` trait for `ReentrantMutex`.");
        // Prose apostrophes, char literals and a lone `&` stay.
        assert_eq!(jux_doc("A handle to a child process's stderr: 'a' & 'b'."), "A handle to a child process's stderr: 'a' & 'b'.");
    }

    #[test]
    fn rust_code_blocks_and_safety_sections_are_dropped() {
        let doc = "Makes a thing.\n\n```rust\nlet x = Thing::new();\n```\n\n# Safety\n\nThe pointer must be valid.\n\n# Examples\n\n```\nlet y = 1;\n```\n\n# Errors\n\nFails when [`Option<u8>`] is empty.";
        assert_eq!(jux_doc(doc), "Makes a thing.\n\n# Errors\n\nFails when `ubyte?` is empty.");
        let kept = "Shows a value.\n```jux\nprint(x);\n```";
        assert_eq!(jux_doc(kept), kept);
    }

    /// Every doc comment of the checked-in `rust.std` stub, and of any stub
    /// under the directories `JUX_TEST_CRATE_STUBS` names (`;`-separated,
    /// e.g. a project's `.jux-stubs/rust` holding egui's), passes the leak
    /// detector once rewritten, with nothing excused. Some fail it as written:
    /// the detector does see rustdoc.
    #[test]
    fn every_real_stub_doc_passes_the_detector_once_rewritten() {
        let std_stub = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../juxc-driver/stubs/rust-std.jux.d");
        let mut files = vec![std_stub];
        if let Ok(dirs) = std::env::var("JUX_TEST_CRATE_STUBS") {
            for dir in dirs.split(';').filter(|d| !d.is_empty()) {
                for entry in std::fs::read_dir(dir).expect("a stub directory").flatten() {
                    if entry.path().to_string_lossy().ends_with(".jux.d") {
                        files.push(entry.path());
                    }
                }
            }
        }
        let (mut docs, mut raw_hits) = (0, 0);
        for file in files {
            let text = std::fs::read_to_string(&file).expect("a stub");
            for line in text.lines() {
                let t = line.trim();
                let Some(doc) = t.strip_prefix("/**").and_then(|d| d.strip_suffix("*/")) else { continue };
                docs += 1;
                let none = |_: &str| false;
                if juxc_diagnostics::leak::find_rust_leak_quoting(doc, &none).is_some() {
                    raw_hits += 1;
                }
                let shown = jux_doc(doc.trim());
                let hit = juxc_diagnostics::leak::find_rust_leak_quoting(&shown, &none);
                assert!(hit.is_none(), "{}: {doc:?}\n -> {shown:?}\n{hit:?}", file.display());
            }
        }
        assert!(docs > 100, "read {docs} docs");
        assert!(raw_hits > 0, "the detector sees rustdoc as written");
    }

    #[test]
    fn the_leak_detector_passes_the_rewritten_text() {
        let lines = [
            "A wrapper around `dyn Any`, used for passing custom user data",
            "Takes `&mut self`, returns `Option<&'static str>`.",
            "See [`std::io::Error`] and `Rc<RefCell<T>>`.",
            "```rust\nlet v: Vec<u8> = vec![];\n```\n# Safety\nunsafe",
        ];
        for line in lines {
            let shown = jux_doc(line);
            assert!(juxc_diagnostics::leak::find_rust_leak(&shown).is_none(), "{line:?} -> {shown:?}");
            assert!(juxc_diagnostics::leak::find_rust_leak_quoting(&shown, &|_| false).is_none(), "{line:?} -> {shown:?}");
        }
    }
}
