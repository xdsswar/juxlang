//! `textDocument/completion`: what can be written at the caret.
//!
//! The entry point, [`build_completions`], is a pure function over the cached
//! document and workspace state, so tests drive it without an LSP client.

use ropey::Rope;
use tower_lsp::lsp_types::*;

use juxc_tycheck::{SymbolTable, Ty};

use crate::analysis::{analyze_single, analyze_workspace};
use crate::calls::{callee_overloads, find_enclosing_call};
use crate::doc::Document;
use crate::imports::{auto_import_for, current_package};
use crate::intel;
use crate::scope::{scope_at, LocalKind, ScopeInfo};
use crate::text::{ident_start_before, is_ident_byte, receiver_dot_before, strip_comments, word_at};
use crate::workspace::Workspace;

/// Jux keywords offered by completion. Java-shaped by design — this is the set
/// an editor colours and suggests. Kept in sync with the lexer's keyword
/// table (`JUX-GRAMMAR-ADDENDUM.md` §A; the lexer is the normative source).
/// Built-in type names offered by completion in every context (a type can name
/// a field, a return type, a local, a cast, …). Coloured as types in the editor.
pub(crate) const PRIMITIVES: &[&str] = &[
    "bool", "char", "byte", "short", "int", "long", "float", "double", "ubyte", "ushort", "uint",
    "ulong", "never", "String", "void",
];

/// Literal constants — only meaningful in expressions (statement context).
pub(crate) const CONSTANTS: &[&str] = &["true", "false", "null"];

/// Keywords valid at the **top level** of a file (package / imports / type
/// declarations and their modifiers).
pub(crate) const TOPLEVEL_KEYWORDS: &[&str] = &[
    "package", "import", "public", "private", "protected", "internal", "abstract", "final",
    "sealed", "static", "const", "class", "interface", "enum", "struct", "record", "annotation",
    "type", "async", "native", "operator", "permits", "extends", "implements",
];

/// Keywords valid inside a **type body** (member declarations + modifiers). No
/// statement keywords, no `print`.
pub(crate) const MEMBER_KEYWORDS: &[&str] = &[
    "public", "private", "protected", "internal", "static", "final", "abstract", "const", "async",
    "operator", "default", "throws", "extends", "implements", "permits",
];

/// Keywords valid inside a **function / method body** (statements & expressions).
pub(crate) const STATEMENT_KEYWORDS: &[&str] = &[
    "var", "return", "if", "else", "for", "while", "do", "switch", "case", "default", "break",
    "continue", "new", "this", "super", "throw", "try", "catch", "finally", "await", "yield", "is",
    "as", "in", "sizeof", "unsafe", "move", "drop",
];

/// Snippets offered at the top level / in a type body (declaration templates).
pub(crate) const DECL_SNIPPETS: &[(&str, &str)] = &[
    ("class", "public class ${1:Name} {\n    $0\n}"),
    ("interface", "public interface ${1:Name} {\n    $0\n}"),
    ("enum", "public enum ${1:Name} {\n    $0\n}"),
    ("struct", "public struct ${1:Name} {\n    $0\n}"),
    ("record", "public record ${1:Name}(${2}) {\n    $0\n}"),
    ("main", "public void main() {\n    $0\n}"),
];

/// Snippets offered inside a function / method body (statement templates).
pub(crate) const STMT_SNIPPETS: &[(&str, &str)] = &[
    ("print", "print($0);"),
    ("if", "if ($1) {\n    $0\n}"),
    ("ifelse", "if ($1) {\n    $2\n} else {\n    $0\n}"),
    ("for", "for (int ${1:i} = 0; $1 < ${2:n}; $1++) {\n    $0\n}"),
    ("while", "while ($1) {\n    $0\n}"),
    ("switch", "switch ($1) {\n    $0\n}"),
    ("return", "return $0;"),
];

/// Where the cursor sits, structurally — drives which completions are offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CtxKind {
    /// Outside any braces — package/imports/type declarations.
    TopLevel,
    /// Inside a `class`/`interface`/`enum`/… body — member declarations.
    TypeBody,
    /// Inside a function/method body or nested block — statements.
    Statement,
}

/// Lexical mode the scanner is in at the cursor — completions are suppressed
/// anywhere but [`ScanMode::Code`] (a name list inside a string literal or a
/// comment is pure noise).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanMode {
    /// Ordinary code — completions apply.
    Code,
    /// Inside a `// …` line comment.
    LineComment,
    /// Inside a `/* … */` block comment.
    BlockComment,
    /// Inside a `"…"` string literal. `interp` marks a `$"…"` interpolated
    /// string, whose `${ … }` holes switch back to [`ScanMode::Code`].
    Str { interp: bool },
    /// Inside a `"""…"""` raw string (or `$"""…"""` raw interpolated string).
    /// Embedded `"` and `\` are content; only `"""` closes it.
    RawStr { interp: bool },
    /// Inside a `'…'` char literal.
    Char,
}

/// Classify the cursor context from the document text **before** the cursor.
///
/// Lightweight and PSI-free: an explicit-mode scan of the prefix tracking a
/// stack of brace "headers". The header that precedes each `{` tells us
/// whether that block is a type body (its header names a `class`/
/// `interface`/…) or a function/statement block. The innermost open block
/// decides the [`CtxKind`]; no open block means top level. The final
/// [`ScanMode`] says whether the cursor itself sits in code or inside a
/// string/comment — the latter suppresses completion entirely.
pub(crate) fn analyze_context(prefix: &str) -> (CtxKind, ScanMode) {
    // Indexed chars so the raw-string arms can look more than one position
    // ahead (`"""` needs two-ahead, which `Peekable` can't give).
    let chars: Vec<char> = prefix.chars().collect();
    // Each stack entry: was the opening brace a type body?
    let mut stack: Vec<bool> = Vec::new();
    let mut seg = String::new();
    let mut mode = ScanMode::Code;
    // For each open `${` interpolation hole: the brace-stack depth at entry
    // plus whether the enclosing string is raw — the matching `}` at that
    // depth returns to the right string mode.
    let mut interp_returns: Vec<(usize, bool)> = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match mode {
            ScanMode::LineComment => {
                if c == '\n' {
                    mode = ScanMode::Code;
                }
            }
            ScanMode::BlockComment => {
                if c == '*' && next == Some('/') {
                    i += 1;
                    mode = ScanMode::Code;
                    seg.push(' ');
                }
            }
            ScanMode::Str { interp } => match c {
                '\\' => {
                    i += 1;
                }
                '"' => {
                    mode = ScanMode::Code;
                    seg.push('"');
                }
                // The lexer terminates a non-raw string at end-of-line, so a
                // mid-edit unclosed quote must not swallow the rest of the
                // file — resync at the newline.
                '\n' => mode = ScanMode::Code,
                '$' if interp && next == Some('{') => {
                    i += 1;
                    interp_returns.push((stack.len(), false));
                    mode = ScanMode::Code;
                }
                _ => {}
            },
            ScanMode::RawStr { interp } => match c {
                // Only `"""` closes a raw string; embedded `"`/`\` are text.
                '"' if next == Some('"') && chars.get(i + 2) == Some(&'"') => {
                    i += 2;
                    mode = ScanMode::Code;
                    seg.push('"');
                }
                '$' if interp && next == Some('{') => {
                    i += 1;
                    interp_returns.push((stack.len(), true));
                    mode = ScanMode::Code;
                }
                _ => {}
            },
            ScanMode::Char => match c {
                '\\' => {
                    i += 1;
                }
                '\'' => mode = ScanMode::Code,
                // Same mid-edit resync rule as strings.
                '\n' => mode = ScanMode::Code,
                _ => {}
            },
            ScanMode::Code => match c {
                '/' => match next {
                    Some('/') => {
                        i += 1;
                        mode = ScanMode::LineComment;
                        // Preserve the pending brace header — `class Foo // x`
                        // with the `{` on the next line is still a type body.
                        seg.push(' ');
                    }
                    Some('*') => {
                        i += 1;
                        mode = ScanMode::BlockComment;
                    }
                    _ => seg.push(c),
                },
                '"' => {
                    // A `$` immediately before the quote marks a `$"…"`
                    // interpolated string (its `${…}` holes are code); a
                    // triple quote opens a raw string.
                    let interp = seg.ends_with('$');
                    if next == Some('"') && chars.get(i + 2) == Some(&'"') {
                        i += 2;
                        mode = ScanMode::RawStr { interp };
                    } else {
                        mode = ScanMode::Str { interp };
                    }
                }
                '\'' => mode = ScanMode::Char,
                '{' => {
                    stack.push(header_is_type(&seg));
                    seg.clear();
                }
                '}' => {
                    if interp_returns.last().map(|(d, _)| *d) == Some(stack.len()) {
                        // This `}` closes a `${…}` hole, not a block — return
                        // to the enclosing (raw or ordinary) string.
                        let raw = interp_returns.pop().map(|(_, r)| r).unwrap_or(false);
                        mode = if raw {
                            ScanMode::RawStr { interp: true }
                        } else {
                            ScanMode::Str { interp: true }
                        };
                    } else {
                        stack.pop();
                        seg.clear();
                    }
                }
                ';' => seg.clear(),
                _ => seg.push(c),
            },
        }
        i += 1;
    }

    let ctx = match stack.last() {
        None => CtxKind::TopLevel,
        Some(true) => CtxKind::TypeBody,
        Some(false) => CtxKind::Statement,
    };
    (ctx, mode)
}

/// True if a brace's preceding header declares a type (so the block is a type
/// body), e.g. `public class Foo<T>`. A function header like
/// `public void main()` has no type keyword and is treated as a statement block.
pub(crate) fn header_is_type(seg: &str) -> bool {
    const TYPE_KW: &[&str] = &["class", "interface", "enum", "struct", "record", "annotation"];
    seg.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| TYPE_KW.contains(&w))
}

/// Recover a receiver expression's type when the **live** analysis couldn't —
/// the typical mid-edit case where the user has typed `obj.` (optionally with a
/// partial member name) but nothing after, so the dangling member access fails
/// to parse and the receiver is left untyped.
///
/// We build a patched buffer in which the `.`-through-caret slice is replaced
/// by a single `;`, turning `… obj.<partial>` into the complete statement
/// `… obj;`. Re-analysing that buffer types the receiver, whose expression span
/// still ends exactly at `recv_end` (the original `.`'s offset) — so the same
/// `end == recv_end` lookup the cache uses finds it. The reparse goes through
/// the *workspace* path when a root is known (so a receiver of a cross-file type
/// resolves too), else the single-file path.
///
/// Only the receiver's `Ty` is taken from the reparse; member resolution still
/// runs against the stable cached symbol table, which always carries the full
/// project + stdlib surface.
pub(crate) fn receiver_type_by_reparse(
    text: &str,
    recv_end: usize,
    caret: usize,
    root: Option<&std::path::Path>,
    uri: &Url,
) -> Option<juxc_tycheck::Ty> {
    // Guard the slice bounds (offsets come from byte scans, but stay defensive).
    if recv_end > caret || caret > text.len() || !text.is_char_boundary(recv_end) || !text.is_char_boundary(caret) {
        return None;
    }
    let mut patched = String::with_capacity(text.len());
    patched.push_str(&text[..recv_end]);
    patched.push(';');
    patched.push_str(&text[caret..]);

    let rope = Rope::from_str(&patched);
    let analysis = match root {
        Some(r) => analyze_workspace(r, uri, &rope),
        None => analyze_single(uri, &rope),
    };
    // The receiver expression now ends at `recv_end` (the `;` we inserted sits
    // right after it); pick the largest span ending there, as the cache does.
    // Only this document's spans: offsets restart at zero in every file.
    let own = analysis.open_file;
    analysis
        .expr_types
        .iter()
        .filter(|(s, _)| own.map_or(true, |f| s.file == f))
        .filter(|(s, _)| s.end as usize == recv_end)
        .max_by_key(|(s, _)| s.len())
        .map(|(_, t)| t.clone())
}

/// `name: ` completions for the parameters of the call the caret sits in.
///
/// Jux takes named arguments (`greet(who: "a", times: 2)`), and nothing offered
/// the names — so using the form meant reading the signature elsewhere first,
/// which is most of the reason to have named arguments in the first place.
///
/// Parameters already written in this call are dropped, so the list shrinks as
/// it is used. These are ADDED to the ordinary completion rather than replacing
/// it: an argument position legitimately takes a value too, and an exclusive
/// branch here would hide every local the user might want to pass.
pub(crate) fn named_argument_items(doc: &Document, text: &str, offset: usize) -> Vec<CompletionItem> {
    let Some((open_paren, _)) = find_enclosing_call(text, offset) else {
        return Vec::new();
    };
    // Only at the START of an argument: once a `:` or any expression character
    // has been typed, the caret is in a value, not a label.
    let arg_text = &text[open_paren + 1..offset];
    let current = arg_text.rsplit(',').next().unwrap_or("");
    if !current.trim_start().chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Vec::new();
    }
    // Names already used in this call — a label written once is spent.
    let used: std::collections::HashSet<&str> = arg_text
        .split(',')
        .filter_map(|a| a.split_once(':').map(|(n, _)| n.trim()))
        .collect();

    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, params) in callee_overloads(doc, text, open_paren) {
        for p in params {
            if used.contains(p.name.as_str()) || !seen.insert(p.name.clone()) {
                continue;
            }
            items.push(CompletionItem {
                label: format!("{}:", p.name),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(intel::render_type(&p.ty)),
                insert_text: Some(format!("{}: ", p.name)),
                // Above locals: at the head of an argument the parameter's own
                // name is the most specific thing that can go there.
                sort_text: Some(format!("00_{}", p.name)),
                ..Default::default()
            });
        }
    }
    items
}

/// When the identifier starting at `ident_start` follows a `case` keyword
/// inside a `switch (…) { … }`, the byte offset just past that switch's
/// SCRUTINEE expression — the input to the same type lookup member completion
/// uses.
///
/// `case Color.| ` already worked: the `Color.` is an ordinary static receiver.
/// What did not was the far more useful bare `case |`, which offered the
/// generic statement bag and left the user to remember both the enum's name and
/// its variants — precisely the two things the compiler already knows here.
///
/// The scan is textual, in two steps, and gives up rather than guessing:
/// backwards from the caret the previous word must be `case`; then backwards
/// again to the `{` that opens the switch body, requiring `)` immediately
/// before it, and the matching `(` is found by depth. A `;` or a nested body
/// ends the search, so a `case` in an unrelated construct cannot reach out to a
/// distant `switch`.
pub(crate) fn switch_scrutinee_before(text: &str, ident_start: usize) -> Option<usize> {
    let bytes = text.as_bytes();

    // Step 1: the word immediately before the caret must be `case`.
    let mut i = ident_start.min(bytes.len());
    while i > 0 && (bytes[i - 1] as char).is_ascii_whitespace() {
        i -= 1;
    }
    let word_end = i;
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    if &text[i..word_end] != "case" {
        return None;
    }

    // Step 2: back to the `{` that opens the enclosing switch body.
    let mut depth = 0i32;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            // A statement terminator at this level means the `case` is not
            // inside a switch body we can read.
            b';' if depth == 0 => return None,
            _ => {}
        }
    }
    if i == 0 || bytes[i] != b'{' {
        return None;
    }

    // Step 3: `{` must be preceded by the switch header's `)`.
    let mut j = i;
    while j > 0 && (bytes[j - 1] as char).is_ascii_whitespace() {
        j -= 1;
    }
    if j == 0 || bytes[j - 1] != b')' {
        return None;
    }
    let scrutinee_end = j - 1;

    // Step 4: the matching `(`, then the `switch` keyword before it.
    let mut k = scrutinee_end;
    let mut paren = 0i32;
    while k > 0 {
        k -= 1;
        match bytes[k] {
            b')' => paren += 1,
            b'(' => {
                if paren == 0 {
                    break;
                }
                paren -= 1;
            }
            _ => {}
        }
    }
    if bytes.get(k) != Some(&b'(') {
        return None;
    }
    let mut w = k;
    while w > 0 && (bytes[w - 1] as char).is_ascii_whitespace() {
        w -= 1;
    }
    let kw_end = w;
    while w > 0 && is_ident_byte(bytes[w - 1]) {
        w -= 1;
    }
    if &text[w..kw_end] != "switch" {
        return None;
    }
    Some(scrutinee_end)
}

/// Completions for a bare `case |`: the scrutinee's enum variants, written the
/// way a pattern has to be written — qualified, `Color.Red`.
///
/// Empty when the scrutinee is not an enum (a `switch` over a string or an int
/// has nothing to enumerate), which lets the caller fall through to the general
/// bag rather than showing an empty popup.
pub(crate) fn case_pattern_items(
    doc: &Document,
    ws: &Workspace,
    uri: &Url,
    text: &str,
    scrutinee_end: usize,
) -> Vec<CompletionItem> {
    // The same type lookup member completion uses: the largest cached span
    // ending at the scrutinee, then a mid-edit reparse as the fallback (the
    // switch body is usually being typed, so the cache can be a keystroke old).
    let ty = match doc.type_ending_at(scrutinee_end).cloned() {
        Some(t) => t,
        None => match receiver_type_by_reparse(
            text,
            scrutinee_end,
            scrutinee_end,
            ws.root.as_deref(),
            uri,
        ) {
            Some(t) => t,
            None => return Vec::new(),
        },
    };
    let juxc_tycheck::Ty::User { name, .. } = ty else {
        return Vec::new();
    };
    let Some(en) = doc.symbols.enums.get(name.as_str()).or_else(|| {
        let bare = name.rsplit('.').next().unwrap_or(name.as_str());
        doc.symbols
            .enums
            .iter()
            .find(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
            .map(|(_, v)| v)
    }) else {
        return Vec::new();
    };
    let bare = name.rsplit('.').next().unwrap_or(name.as_str()).to_string();
    let mut variants: Vec<&String> = en.variants.keys().collect();
    variants.sort();
    variants
        .into_iter()
        .map(|v| {
            let label = format!("{bare}.{v}");
            CompletionItem {
                label: label.clone(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                detail: Some(bare.clone()),
                // Every variant is equally likely, so one flat tier; the
                // client's prefix matching does the rest.
                sort_text: Some(format!("0_{label}")),
                ..Default::default()
            }
        })
        .collect()
}

/// True when the identifier starting at `ident_start` is preceded directly by
/// `@` — an annotation name.
///
/// Deliberately no whitespace skip: `@ Test` is not annotation syntax, and
/// treating it as such would offer annotations after every stray `@`.
pub(crate) fn at_sign_before(text: &str, ident_start: usize) -> bool {
    ident_start > 0 && text.as_bytes()[ident_start - 1] == b'@'
}

/// Annotation-name completions.
///
/// Only the built-ins: the compiler does not parse a user `annotation`
/// declaration at all (it is a reserved form), so there is nothing else that
/// could legally follow an `@`. The list is generated from the compiler's own
/// [`juxc_lex::grammar_spec::BUILTIN_ANNOTATIONS`], so an annotation that gains
/// behaviour cannot go missing here; a drift test in that module pins it.
pub(crate) fn annotation_items() -> Vec<CompletionItem> {
    juxc_lex::grammar_spec::BUILTIN_ANNOTATIONS
        .iter()
        .map(|name| CompletionItem {
            label: (*name).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("built-in annotation".to_string()),
            sort_text: Some(format!("0_{name}")),
            ..Default::default()
        })
        .collect()
}

/// A cursor position where ONLY type names make sense — completion narrows to
/// the kinds each position can legally take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypePos {
    /// `new <Type>` — instantiable types: non-abstract classes + records.
    New,
    /// `class C extends <Type>` (`interfaces: false` → non-final classes),
    /// `interface I extends <Type>` (`interfaces: true` → interfaces), or a
    /// generic bound `<T extends <Type>>` (`generic: true` → both).
    Extends { interfaces: bool, generic: bool },
    /// `implements <Type>` — interfaces only.
    Implements,
}

/// Detect whether the identifier starting at `ident_start` sits in a
/// type-only position: after `new`, or in the type list of an
/// `extends`/`implements` clause (including past commas — `implements A, B`)
/// or a generic bound (`<T extends Comparable>`).
///
/// The clause check scans back to the nearest statement/header boundary
/// (`;`/`{`/`}`/`(`/`)`/`=`), finds the LAST `extends`/`implements` keyword in
/// that slice, and requires everything after the keyword to be type-list
/// shaped (idents, commas, dots, generics, whitespace) so an `extends` deep in
/// an unrelated header doesn't trigger.
pub(crate) fn type_position(text: &str, ident_start: usize) -> Option<TypePos> {
    let bytes = text.as_bytes();
    let mut s = ident_start.min(bytes.len());
    while s > 0 && !matches!(bytes[s - 1], b';' | b'{' | b'}' | b'(' | b')' | b'=') {
        s -= 1;
    }
    // Blank out comments first so a keyword INSIDE a comment (`// extends X,`)
    // can't fake a type position for the statement typed below it.
    let slice = strip_comments(&text[s..ident_start]);
    let slice = slice.as_str();

    // `new <Type>` fast path: the last word before the cursor is `new`.
    // (Byte-level compare — never slices mid-char.)
    let tb = slice.trim_end().as_bytes();
    if tb.len() >= 3
        && &tb[tb.len() - 3..] == b"new"
        && (tb.len() == 3 || !is_ident_byte(tb[tb.len() - 4]))
    {
        return Some(TypePos::New);
    }

    // Word-boundary `rfind` of a keyword inside `slice`; returns the offset
    // just past the keyword.
    let find_kw = |kw: &str| -> Option<usize> {
        let mut from = slice.len();
        while let Some(p) = slice[..from].rfind(kw) {
            let before_ok = p == 0 || !is_ident_byte(slice.as_bytes()[p - 1]);
            let after = p + kw.len();
            let after_ok = after >= slice.len() || !is_ident_byte(slice.as_bytes()[after]);
            if before_ok && after_ok {
                return Some(after);
            }
            from = p;
        }
        None
    };
    // The tail between the keyword and the cursor must look like a type list
    // AND still be "open" at the cursor: right after the keyword, or after a
    // separator (`,` `<` `&` `.`). After a COMPLETE type name plus a space
    // (`extends Bar |`) the next word is `implements` / `{`, not another
    // type, so the position is no longer type-only.
    let tail_ok = |from: usize| {
        let tail = &slice[from..];
        let shape_ok = tail.chars().all(|c| {
            c.is_alphanumeric()
                || c == '_'
                || c.is_whitespace()
                || matches!(c, ',' | '.' | '<' | '>' | '?' | '&')
        });
        let open = match tail.trim_end().chars().last() {
            None => true,
            Some(c) => matches!(c, ',' | '<' | '&' | '.' | '?'),
        };
        shape_ok && open
    };
    let has_word = |kw: &str| find_kw(kw).is_some();

    let ext = find_kw("extends").filter(|&p| tail_ok(p));
    let imp = find_kw("implements").filter(|&p| tail_ok(p));
    match (ext, imp) {
        // Both present (a full `class C extends P implements A` header):
        // whichever clause the cursor's list belongs to is the LATER keyword.
        (Some(e), Some(i)) => {
            if i > e {
                Some(TypePos::Implements)
            } else {
                Some(extends_pos(slice, e, has_word("interface")))
            }
        }
        (Some(e), None) => Some(extends_pos(slice, e, has_word("interface"))),
        (None, Some(_)) => Some(TypePos::Implements),
        (None, None) => None,
    }
}

/// Classify an `extends` hit: an unclosed `<` before the keyword means a
/// generic bound (`<T extends |`, both classes and interfaces are legal);
/// otherwise the header's own kind decides (interface headers extend
/// interfaces, class headers extend classes).
pub(crate) fn extends_pos(slice: &str, kw_end: usize, in_interface: bool) -> TypePos {
    let before = &slice[..kw_end];
    let opens = before.matches('<').count();
    let closes = before.matches('>').count();
    TypePos::Extends { interfaces: in_interface, generic: opens > closes }
}

/// When the caret sits on an `import …` line, the dotted path typed so far
/// (after the keyword, up to the caret). `None` anywhere else — including
/// while the `import` keyword itself is still being typed. Only the text
/// after the line's last `;` counts, so `import a.B; <code>` doesn't put the
/// trailing code in import-path mode (nor vice versa).
pub(crate) fn import_prefix(text: &str, offset: usize) -> Option<String> {
    let line_start = text[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line = &text[line_start..offset];
    let line = line.rsplit(';').next().unwrap_or(line);
    let rest = line.trim_start().strip_prefix("import")?;
    // Require the space after the keyword (`import|` is still the keyword).
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim_start().to_string())
}

// ============================================================================
// Completion — item construction
//
// One ranking scheme, encoded in `sort_text` prefixes (LSP clients sort by
// it when present):
//   0_  locals / parameters; after-dot members of the receiver
//   1_  implicit-`this` members of the enclosing class; type-position items
//   2_  type names (open file + workspace, with auto-import) and functions
//   3_  snippets, built-in types, literal constants
//   4_  keywords
//   5_  the flat cross-project member-name bag
// ============================================================================

/// Resolve the FQN key of the class whose body contains the caret, so member
/// visibility (`private`/`protected`) is judged from the right place. The
/// bare→FQN map prefers a same-package declaration, mirroring the checker.
pub(crate) fn enclosing_fqn(symbols: &SymbolTable, scope: &ScopeInfo, pkg: Option<&str>) -> Option<String> {
    let name = scope.enclosing_class.as_ref()?;
    symbols
        .find_fqn_by_bare_in(name, pkg.unwrap_or(""))
        .or_else(|| Some(name.clone()))
}

/// The storage key of the type `ident` names, if any — used to recognize a
/// `Type.` receiver (statics + enum variants) without re-analysing anything.
pub(crate) fn type_key_for(symbols: &SymbolTable, ident: &str, pkg: Option<&str>) -> Option<String> {
    if symbols.classes.contains_key(ident)
        || symbols.enums.contains_key(ident)
        || symbols.records.contains_key(ident)
        || symbols.interfaces.contains_key(ident)
    {
        return Some(ident.to_string());
    }
    symbols.find_fqn_by_bare_in(ident, pkg.unwrap_or(""))
}

/// Build one completion item from a resolved [`intel::Member`].
///
/// Methods insert a parameter snippet — `greet(${1:who})` leaves the caret on
/// the first argument; a no-arg method inserts `name()` with the caret after
/// the parens. Properties / fields / unit variants insert the bare name;
/// payload variants insert `Name($1)`. The `data` payload feeds
/// `completionItem/resolve`, which attaches the declaration's doc comment.
pub(crate) fn member_item(m: intel::Member, uri: &Url, sort_prefix: &str) -> CompletionItem {
    use intel::MemberKind;
    let kind = match m.kind {
        MemberKind::Method => CompletionItemKind::METHOD,
        MemberKind::Property => CompletionItemKind::PROPERTY,
        MemberKind::Field => CompletionItemKind::FIELD,
        MemberKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
    };
    let (insert_text, insert_text_format) = match m.kind {
        MemberKind::Method => match m.params.as_deref() {
            Some([]) | None => (Some(format!("{}()", m.name)), None),
            Some(params) => {
                // Placeholder text is the parameter name — plain identifiers,
                // so no snippet-escaping is needed.
                let holes = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("${{{}:{}}}", i + 1, p))
                    .collect::<Vec<_>>()
                    .join(", ");
                (Some(format!("{}({holes})", m.name)), Some(InsertTextFormat::SNIPPET))
            }
        },
        MemberKind::EnumVariant if m.params.is_some() => {
            (Some(format!("{}($1)", m.name)), Some(InsertTextFormat::SNIPPET))
        }
        _ => (None, None),
    };
    let data = serde_json::json!({
        "uri": uri.to_string(),
        "owner": m.owner_fqn,
        "member": m.name,
    });
    CompletionItem {
        label: m.name.clone(),
        kind: Some(kind),
        detail: Some(m.detail),
        filter_text: Some(m.name.clone()),
        sort_text: Some(format!("{sort_prefix}{}", m.name)),
        insert_text,
        insert_text_format,
        data: Some(data),
        ..Default::default()
    }
}

/// Items for an `import …` line: the next dotted segment of every known type
/// FQN extending what's typed so far. Package segments come first (MODULE),
/// terminal type names after (CLASS, detail = the full FQN).
pub(crate) fn import_items(symbols: &SymbolTable, prefix: &str) -> Vec<CompletionItem> {
    let base = prefix.rsplit_once('.').map(|(b, _)| b).unwrap_or("");
    let mut seen: std::collections::HashSet<(String, bool)> = std::collections::HashSet::new();
    let mut items = Vec::new();
    let keys: Vec<&String> = symbols
        .classes
        .keys()
        .chain(symbols.enums.keys())
        .chain(symbols.records.keys())
        .chain(symbols.interfaces.keys())
        .collect();
    // How many distinct dotted FQNs share each bare name — the bare-name
    // shortcut below must not silently pick one of several candidates.
    let mut bare_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for fqn in &keys {
        if fqn.contains('.') {
            let bare = fqn.rsplit('.').next().unwrap_or(fqn);
            *bare_counts.entry(bare).or_insert(0) += 1;
        }
    }
    for fqn in keys {
        // Package-less types can't be imported — nothing to offer.
        if !fqn.contains('.') {
            continue;
        }
        let rel = if base.is_empty() {
            fqn.as_str()
        } else {
            match fqn.strip_prefix(base).and_then(|r| r.strip_prefix('.')) {
                Some(r) => r,
                None => continue,
            }
        };
        let (seg, terminal) = match rel.split_once('.') {
            Some((s, _)) => (s, false),
            None => (rel, true),
        };
        if seg.is_empty() || !seen.insert((seg.to_string(), terminal)) {
            continue;
        }
        items.push(CompletionItem {
            label: seg.to_string(),
            kind: Some(if terminal { CompletionItemKind::CLASS } else { CompletionItemKind::MODULE }),
            detail: terminal.then(|| fqn.clone()),
            // Packages before types; alphabetical within each group.
            sort_text: Some(format!("{}_{seg}", if terminal { 1 } else { 0 })),
            ..Default::default()
        });
        // Java-IDE nicety: with no package typed yet, also offer the TYPE by
        // its bare name, inserting the whole dotted path — `import Wid` →
        // `import xss.it.Widget`. Only when the bare name is UNAMBIGUOUS:
        // with several declaring packages, silently inserting one of them
        // would be a wrong guess half the time.
        if base.is_empty() && !terminal {
            let bare = fqn.rsplit('.').next().unwrap_or(fqn);
            if bare_counts.get(bare) == Some(&1) && seen.insert((bare.to_string(), true)) {
                items.push(CompletionItem {
                    label: bare.to_string(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some(fqn.clone()),
                    insert_text: Some(fqn.clone()),
                    sort_text: Some(format!("1_{bare}")),
                    ..Default::default()
                });
            }
        }
    }
    items
}

/// Items for a type-only position (`new` / `extends` / `implements`): just
/// the type names that position can legally take, inserted bare, each with
/// an auto-import edit when applicable.
pub(crate) fn type_position_items(
    doc: &Document,
    ws: &Workspace,
    uri: &Url,
    text: &str,
    pos: TypePos,
) -> Vec<CompletionItem> {
    let symbols = &doc.symbols;
    let cur_pkg = current_package(text).or_else(|| doc.inferred_package.clone());
    let mut names: Vec<(String, CompletionItemKind)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let add = |fqn: &str,
               kind: CompletionItemKind,
                   names: &mut Vec<(String, CompletionItemKind)>,
                   seen: &mut std::collections::HashSet<String>| {
        let bare = fqn.rsplit('.').next().unwrap_or(fqn).to_string();
        if seen.insert(bare.clone()) {
            names.push((bare, kind));
        }
    };

    match pos {
        // `new` — instantiable types only: concrete classes + records.
        TypePos::New => {
            for (k, sig) in symbols.classes.iter() {
                if !sig.is_abstract {
                    add(k, CompletionItemKind::CLASS, &mut names, &mut seen);
                }
            }
            for k in symbols.records.keys() {
                add(k, CompletionItemKind::STRUCT, &mut names, &mut seen);
            }
        }
        // `implements` — interfaces only.
        TypePos::Implements => {
            for k in symbols.interfaces.keys() {
                add(k, CompletionItemKind::INTERFACE, &mut names, &mut seen);
            }
        }
        // `extends` — what may be extended depends on where the clause sits.
        TypePos::Extends { interfaces, generic } => {
            if interfaces || generic {
                for k in symbols.interfaces.keys() {
                    add(k, CompletionItemKind::INTERFACE, &mut names, &mut seen);
                }
            }
            if !interfaces || generic {
                for (k, sig) in symbols.classes.iter() {
                    // Final classes can't be extended (E0420); foreign stub
                    // classes can't be subclassed either.
                    if !sig.is_final && !sig.is_external {
                        add(k, CompletionItemKind::CLASS, &mut names, &mut seen);
                    }
                }
            }
        }
    }

    names.sort_by(|a, b| a.0.cmp(&b.0));
    names
        .into_iter()
        .map(|(name, kind)| {
            let import = auto_import_for(&name, ws, cur_pkg.as_deref(), &doc.rope);
            let detail = import.as_ref().map(|_| format!("auto-imports {name}"));
            let data = serde_json::json!({
                "uri": uri.to_string(),
                "owner": name,
                "member": null,
            });
            CompletionItem {
                label: name.clone(),
                kind: Some(kind),
                detail,
                sort_text: Some(format!("1_{name}")),
                additional_text_edits: import.map(|e| vec![e]),
                data: Some(data),
                ..Default::default()
            }
        })
        .collect()
}

/// Member completion for `<receiver>.<partial>` — classify how the receiver
/// was written, resolve its type, and return ONLY its accessible members.
pub(crate) fn member_completions(
    doc: &Document,
    ws: &Workspace,
    uri: &Url,
    text: &str,
    recv_end: usize,
    caret: usize,
) -> Vec<CompletionItem> {
    // Scope picture at the receiver (enclosing class for visibility; locals
    // to tell a variable receiver from a type-name receiver).
    let own_types = doc.own_expr_types();
    let scope = scope_at(text, recv_end, caret, &own_types);
    let cur_pkg = current_package(text).or_else(|| doc.inferred_package.clone());
    let access = intel::AccessCtx {
        package: cur_pkg.clone(),
        enclosing_class_fqn: enclosing_fqn(&doc.symbols, &scope, cur_pkg.as_deref()),
    };

    // Classify the receiver from the identifier ending at the `.`.
    let recv_word = word_at(text, recv_end);
    let mut resolved: Option<(Ty, intel::ReceiverKind)> = None;
    if let Some(w) = &recv_word {
        let is_local = scope.locals.iter().any(|l| l.name == w.text);
        if w.text == "this" {
            // `this.` — the enclosing class, instance members (the access
            // context already grants private).
            if let Some(fqn) = &access.enclosing_class_fqn {
                resolved = Some((
                    Ty::User { name: fqn.clone(), generic_args: vec![] },
                    intel::ReceiverKind::Instance,
                ));
            }
        } else if w.text == "super" {
            // `super.` — the parent class, instance members.
            if let Some(parent) = access
                .enclosing_class_fqn
                .as_ref()
                .and_then(|fqn| doc.symbols.classes.get(fqn))
                .and_then(|c| c.extends_fqn.clone())
            {
                resolved = Some((
                    Ty::User { name: parent, generic_args: vec![] },
                    intel::ReceiverKind::Instance,
                ));
            }
        } else if !is_local && receiver_dot_before(text, w.start).is_none() {
            // A bare type name (`Color.` / `Math.`): statics + enum variants.
            // Locals shadow type names, hence the `is_local` guard — and a
            // chained receiver (`a.B.`) stays on the expression path.
            if let Some(key) = type_key_for(&doc.symbols, &w.text, cur_pkg.as_deref()) {
                resolved = Some((
                    Ty::User { name: key, generic_args: vec![] },
                    intel::ReceiverKind::Static,
                ));
            }
        }
    }

    // Expression receiver: the cached analysis first, then the mid-edit
    // reparse fallback (`obj.` patched to `obj;`).
    let (ty, kind) = match resolved {
        Some(r) => r,
        None => {
            let recovered = doc.type_ending_at(recv_end).cloned().or_else(|| {
                receiver_type_by_reparse(text, recv_end, caret, ws.root.as_deref(), uri)
            });
            match recovered {
                Some(t) => (t, intel::ReceiverKind::Instance),
                // A member-access context whose receiver we couldn't resolve:
                // the statement keyword/snippet bag would be pure noise after
                // `obj.`, so return nothing.
                None => return Vec::new(),
            }
        }
    };

    intel::members_of(&doc.symbols, &ty, kind, &access)
        .into_iter()
        .map(|m| member_item(m, uri, "0_"))
        .collect()
}

/// The completion entry point — a pure function over the cached document +
/// workspace state, so unit tests drive it directly (no async, no `Client`).
pub(crate) fn build_completions(doc: &Document, ws: &Workspace, uri: &Url, offset: usize) -> Vec<CompletionItem> {
    let text = doc.rope.to_string();
    let offset = offset.min(text.len());

    // Where is the cursor, structurally — and is it even in code? No
    // completions inside strings, comments, or char literals.
    let (ctx, mode) = analyze_context(&text[..offset]);
    if mode != ScanMode::Code {
        return Vec::new();
    }

    // `import a.b.|` — complete the package path / type name and nothing else.
    if let Some(prefix) = import_prefix(&text, offset) {
        return import_items(&doc.symbols, &prefix);
    }

    // The partial word being completed.
    let member_start = ident_start_before(&text, offset);
    let typed_prefix = text[member_start..offset].to_string();

    // `case |` — the scrutinee's own variants, exclusively. See
    // `switch_scrutinee_before`.
    if let Some(recv_end) = switch_scrutinee_before(&text, member_start) {
        let items = case_pattern_items(doc, ws, uri, &text, recv_end);
        if !items.is_empty() {
            return items;
        }
    }

    // `@|` — an annotation name, exclusively. `@` is one of this server's
    // trigger characters, so every `@` the user types asks for completion; with
    // no branch here it fell through to the general bag and offered keywords and
    // locals. The IntelliJ plugin has an annotation list of its own, but it
    // stands down entirely while this server is serving, so `@` offered nothing
    // useful in the one configuration most users run.
    if at_sign_before(&text, member_start) {
        return annotation_items();
    }

    // `<receiver>.` — member completion, exclusively.
    if let Some(recv_end) = receiver_dot_before(&text, member_start) {
        return member_completions(doc, ws, uri, &text, recv_end, offset);
    }

    // `new <T>` / `extends <T>` / `implements <T>` — type names, exclusively.
    if let Some(pos) = type_position(&text, member_start) {
        return type_position_items(doc, ws, uri, &text, pos);
    }

    let mut items: Vec<CompletionItem> = Vec::new();
    let cur_pkg = current_package(&text).or_else(|| doc.inferred_package.clone());

    // `f(|` — the callee's parameter names, as `name:` labels. Added rather
    // than exclusive: an argument position takes a value too.
    items.extend(named_argument_items(doc, &text, offset));

    // Scope-aware names — the things the user most likely wants to type:
    // locals + parameters first, then the enclosing class's own members
    // (implicit `this`), statics-only inside a static method. The walk
    // (lex + parse) only runs where its results are used.
    let scope = if ctx == CtxKind::Statement {
        scope_at(&text, member_start, offset, &doc.own_expr_types())
    } else {
        ScopeInfo::default()
    };
    if ctx == CtxKind::Statement && scope.in_fn_body {
        let mut preselected = false;
        for var in &scope.locals {
            let detail = var.ty_display.clone().unwrap_or_else(|| {
                match var.kind {
                    LocalKind::Param => "parameter",
                    LocalKind::Local => "local",
                    LocalKind::ForEachVar => "loop variable",
                    LocalKind::CatchVar => "caught exception",
                }
                .to_string()
            });
            // Pre-select the first local matching what's typed — the single
            // most likely intent.
            let preselect = !preselected
                && !typed_prefix.is_empty()
                && var.name.starts_with(&typed_prefix);
            preselected |= preselect;
            items.push(CompletionItem {
                label: var.name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(detail),
                sort_text: Some(format!("0_{}", var.name)),
                preselect: preselect.then_some(true),
                ..Default::default()
            });
        }
        if let Some(fqn) = enclosing_fqn(&doc.symbols, &scope, cur_pkg.as_deref()) {
            let access = intel::AccessCtx {
                package: cur_pkg.clone(),
                enclosing_class_fqn: Some(fqn.clone()),
            };
            let ty = Ty::User { name: fqn, generic_args: vec![] };
            let kind = intel::ReceiverKind::Implicit {
                in_static_method: scope.enclosing_fn_is_static,
            };
            for m in intel::members_of(&doc.symbols, &ty, kind, &access) {
                items.push(member_item(m, uri, "1_"));
            }
        }
    }

    // Snippets — declaration templates at top level / type body, statement
    // templates inside a function body.
    let snippets: &[(&str, &str)] = match ctx {
        CtxKind::Statement => STMT_SNIPPETS,
        CtxKind::TopLevel | CtxKind::TypeBody => DECL_SNIPPETS,
    };
    for (label, body) in snippets {
        items.push(CompletionItem {
            label: (*label).to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("snippet".to_string()),
            insert_text: Some((*body).to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("3_{label}")),
            ..Default::default()
        });
    }

    // Keywords for this context.
    let keywords: &[&str] = match ctx {
        CtxKind::TopLevel => TOPLEVEL_KEYWORDS,
        CtxKind::TypeBody => MEMBER_KEYWORDS,
        CtxKind::Statement => STATEMENT_KEYWORDS,
    };
    for kw in keywords {
        items.push(CompletionItem {
            label: (*kw).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            sort_text: Some(format!("4_{kw}")),
            ..Default::default()
        });
    }

    // Built-in types — a type can name a field, a return type, a local, …
    for ty in PRIMITIVES {
        items.push(CompletionItem {
            label: (*ty).to_string(),
            kind: Some(CompletionItemKind::STRUCT),
            detail: Some("built-in type".to_string()),
            sort_text: Some(format!("3_{ty}")),
            ..Default::default()
        });
    }

    // Literal constants — expressions only.
    if ctx == CtxKind::Statement {
        for c in CONSTANTS {
            items.push(CompletionItem {
                label: (*c).to_string(),
                kind: Some(CompletionItemKind::CONSTANT),
                sort_text: Some(format!("3_{c}")),
                ..Default::default()
            });
        }
    }

    // Track labels already added so later (coarser) sources don't duplicate
    // the scope-aware / keyword items above.
    let mut seen: std::collections::HashSet<String> =
        items.iter().map(|i| i.label.clone()).collect();

    // Type names from the open file's live analysis (fresh, includes types
    // just typed but not yet saved) + the project-wide index, each with an
    // auto-import edit when the type's package is unambiguous.
    let type_data = |name: &str| {
        serde_json::json!({ "uri": uri.to_string(), "owner": name, "member": null })
    };
    for name in &doc.type_names {
        if seen.insert(name.clone()) {
            // Attach the same auto-import edit workspace types get: a type
            // surfaced by the file's live analysis that actually lives in
            // another (unambiguous) package still needs its `import` on accept.
            // For genuinely same-file / same-package names `auto_import_for`
            // returns None, so this stays a no-op there.
            let import = auto_import_for(name, ws, cur_pkg.as_deref(), &doc.rope);
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::CLASS),
                detail: import.as_ref().map(|_| format!("auto-imports {name}")),
                sort_text: Some(format!("2_{name}")),
                additional_text_edits: import.map(|e| vec![e]),
                data: Some(type_data(name)),
                ..Default::default()
            });
        }
    }
    for name in &ws.type_names {
        if seen.insert(name.clone()) {
            let import = auto_import_for(name, ws, cur_pkg.as_deref(), &doc.rope);
            let detail = match &import {
                Some(_) => Some(format!("project type -- auto-imports {name}")),
                None => Some("project type".to_string()),
            };
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::CLASS),
                detail,
                sort_text: Some(format!("2_{name}")),
                additional_text_edits: import.map(|e| vec![e]),
                data: Some(type_data(name)),
                ..Default::default()
            });
        }
    }

    // The flat cross-project member bag — last-resort recall for names whose
    // receiver/scope we couldn't tie down. Expression positions only.
    if ctx == CtxKind::Statement {
        for name in &ws.member_names {
            if seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some("project member".to_string()),
                    sort_text: Some(format!("5_{name}")),
                    ..Default::default()
                });
            }
        }
    }

    items
}

/// Lazy completion documentation (`completionItem/resolve`): the `data`
/// payload attached at completion time identifies the declaring symbol; here we
/// locate its declaration (possibly in another file or a generated stub) and
/// attach its doc comment, paying the file read only for the item the user
/// actually highlights. `doc_for` finds the open document the item came from.
pub(crate) fn resolve_item(
    mut item: CompletionItem,
    doc_for: &dyn Fn(&Url) -> Option<std::sync::Arc<DocSnapshot>>,
) -> CompletionItem {
    let Some(data) = item.data.clone() else { return item };
    let Some(obj) = data.as_object() else { return item };
    let (Some(uri_s), Some(owner)) = (
        obj.get("uri").and_then(|v| v.as_str()),
        obj.get("owner").and_then(|v| v.as_str()),
    ) else {
        return item;
    };
    let member = obj.get("member").and_then(|v| v.as_str());
    let Ok(uri) = Url::parse(uri_s) else { return item };
    let Some(snap) = doc_for(&uri) else { return item };
    let symbols = &snap.symbols;

    // Locate the declaration: a member's span inside its owner type, or the
    // owner declaration itself (types, functions; bare names ok).
    let located = match member {
        Some(m) => intel::member_decl_span(symbols, owner, m)
            .and_then(|span| symbols.decl_unit.get(owner).map(|&u| (u, span))),
        None => symbols.definition_of(owner),
    };
    let Some((unit, span)) = located else { return item };
    let Some(path) = snap.source_paths.get(unit) else { return item };

    // The declaring file's text: the live buffer when it's the open document,
    // else from disk (covers generated `.jux.d` stubs too).
    let same_as_open = Url::from_file_path(path).ok().as_ref() == Some(&uri);
    let decl_text = if same_as_open {
        snap.text.clone()
    } else {
        match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return item,
        }
    };
    if let Some(doc_line) = crate::text::doc_comment_before(&decl_text, span.start as usize) {
        item.documentation = Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc_line,
        }));
    }
    item
}

/// What [`resolve_item`] needs of an open document: its text and the symbol
/// table it was analysed with. A snapshot, so no store lock is held while the
/// declaring file is read from disk.
pub(crate) struct DocSnapshot {
    /// The live buffer text.
    pub(crate) text: String,
    /// The document's symbol table.
    pub(crate) symbols: std::sync::Arc<SymbolTable>,
    /// Paths parallel to the symbol table's unit indices.
    pub(crate) source_paths: std::sync::Arc<Vec<std::path::PathBuf>>,
}
