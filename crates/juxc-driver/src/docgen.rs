//! `jux doc`: API documentation from doc comments (JUX-LANG-V1 §3.5, §12.5;
//! JUX-BUILD-SYSTEM-ADDENDUM §B.14.8, §B.15.4).
//!
//! A doc comment is a `/** ... */` block written directly in front of a
//! declaration (annotations may sit between the two). Its text is Markdown;
//! the tags `@param`, `@return`, `@throws`, `@deprecated`, `@since` and `@see`
//! are pulled out into their own sections. Fenced code blocks tagged `jux` are
//! examples, and examples are tests: [`run_doctests`] compiles and runs each
//! one against the package, so a doc example that stops working fails the
//! build instead of misleading the reader.
//!
//! The generator reads declarations from the AST (so every public item is
//! listed, documented or not) and reads the comment and the signature from
//! the source text in front of and at the declaration's span. The result is a
//! small static site under `target/doc/`: an index with a search box and the
//! package's dependencies, and one page per Jux package with every public
//! type, function and constant, their members, and cross-links between
//! them. Nothing is fetched from the network; the pages work from disk.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use juxc_ast::{TopLevelDecl, Visibility};
use juxc_source::SourceFile;

use crate::manifest::Manifest;

/// The parsed pieces of one doc comment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocComment {
    /// The Markdown body with the tag lines removed.
    pub body: String,
    /// `@param name text`, in order.
    pub params: Vec<(String, String)>,
    /// `@return text`.
    pub returns: Option<String>,
    /// `@throws Type text`, in order.
    pub throws: Vec<(String, String)>,
    /// `@deprecated reason` (an empty reason still marks the item).
    pub deprecated: Option<String>,
    /// `@since version`.
    pub since: Option<String>,
    /// `@see target`, in order.
    pub see: Vec<String>,
}

impl DocComment {
    /// The first paragraph of the body, as one line: what an index shows.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for line in self.body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("```") || line.starts_with('#') {
                if !out.is_empty() {
                    break;
                }
                if line.starts_with("```") || line.starts_with('#') {
                    break;
                }
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(line);
        }
        out
    }

    /// Every fenced code block tagged `jux` in the body, with its info string
    /// (`jux`, `jux ignore`, `jux no_run`).
    pub fn examples(&self) -> Vec<DocExample> {
        let mut out = Vec::new();
        let mut current: Option<(String, Vec<String>)> = None;
        for line in self.body.lines() {
            let trimmed = line.trim_start();
            if let Some(info) = trimmed.strip_prefix("```") {
                match current.take() {
                    Some((info, lines)) => {
                        let mut words = info.split_whitespace();
                        if words.next() == Some("jux") {
                            let flags: Vec<&str> = words.collect();
                            out.push(DocExample {
                                code: lines.join("\n"),
                                ignore: flags.contains(&"ignore"),
                                no_run: flags.contains(&"no_run"),
                            });
                        }
                    }
                    None => current = Some((info.trim().replace(',', " "), Vec::new())),
                }
                continue;
            }
            if let Some((_, lines)) = current.as_mut() {
                lines.push(line.to_string());
            }
        }
        out
    }
}

/// One ```` ```jux ```` block from a doc comment.
#[derive(Debug, Clone, PartialEq)]
pub struct DocExample {
    /// The code between the fences.
    pub code: String,
    /// `jux ignore`: shown, never compiled.
    pub ignore: bool,
    /// `jux no_run`: compiled, not run.
    pub no_run: bool,
}

/// Split a raw `/** ... */` comment into body and tags. The comment markers
/// and each line's leading `*` are removed first; a tag's text continues
/// onto following lines until a blank line or the next tag.
pub fn parse_doc_comment(raw: &str) -> DocComment {
    let inner = raw
        .trim()
        .trim_start_matches("/**")
        .trim_end_matches("*/");
    let lines: Vec<String> = inner
        .lines()
        .map(|l| {
            let t = l.trim_start();
            let t = t.strip_prefix('*').unwrap_or(t);
            // One space after the `*` is the comment's margin, not content.
            t.strip_prefix(' ').unwrap_or(t).trim_end().to_string()
        })
        .collect();

    let mut doc = DocComment::default();
    let mut body: Vec<String> = Vec::new();
    // The tag being continued: (tag, index into its list) as a tiny state.
    let mut open: Option<String> = None;
    let mut in_fence = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            open = None;
            body.push(line);
            continue;
        }
        if !in_fence && trimmed.starts_with('@') {
            let (tag, rest) = trimmed[1..].split_once(char::is_whitespace).unwrap_or((&trimmed[1..], ""));
            let rest = rest.trim().to_string();
            match tag {
                "param" => {
                    let (name, text) = rest.split_once(char::is_whitespace).unwrap_or((&rest, ""));
                    doc.params.push((name.to_string(), text.trim().to_string()));
                }
                "return" | "returns" => doc.returns = Some(rest),
                "throws" | "exception" => {
                    let (ty, text) = rest.split_once(char::is_whitespace).unwrap_or((&rest, ""));
                    doc.throws.push((ty.to_string(), text.trim().to_string()));
                }
                "deprecated" => doc.deprecated = Some(rest),
                "since" => doc.since = Some(rest),
                "see" => doc.see.push(rest),
                // An unknown tag stays in the body as written.
                _ => {
                    body.push(line);
                    open = None;
                    continue;
                }
            }
            open = Some(tag.to_string());
            continue;
        }
        if let Some(tag) = &open {
            if trimmed.is_empty() {
                open = None;
            } else {
                let more = format!(" {trimmed}");
                match tag.as_str() {
                    "param" => {
                        if let Some(last) = doc.params.last_mut() {
                            last.1.push_str(&more);
                        }
                    }
                    "return" | "returns" => {
                        if let Some(r) = doc.returns.as_mut() {
                            r.push_str(&more);
                        }
                    }
                    "throws" | "exception" => {
                        if let Some(last) = doc.throws.last_mut() {
                            last.1.push_str(&more);
                        }
                    }
                    "deprecated" => {
                        if let Some(d) = doc.deprecated.as_mut() {
                            d.push_str(&more);
                        }
                    }
                    _ => {}
                }
                continue;
            }
        }
        body.push(line);
    }
    // Trim leading and trailing blank lines of the body.
    while body.first().is_some_and(|l| l.trim().is_empty()) {
        body.remove(0);
    }
    while body.last().is_some_and(|l| l.trim().is_empty()) {
        body.pop();
    }
    doc.body = body.join("\n");
    doc
}

/// The doc comment written in front of `start` in `text`, if any: skip back
/// over whitespace and annotation lines, then take a `/** ... */` block that
/// ends right there.
pub fn doc_comment_before(text: &str, start: usize) -> Option<String> {
    if start > text.len() || !text.is_char_boundary(start) {
        return None;
    }
    let mut end = start;
    loop {
        let before = text[..end].trim_end();
        if before.ends_with("*/") {
            let open = before.rfind("/*")?;
            let comment = &before[open..];
            return comment.starts_with("/**").then(|| comment.to_string());
        }
        // An annotation line (`@Deprecated`, `@cfg(os = "x")`) between the
        // comment and the declaration belongs to the declaration.
        let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line = before[line_start..].trim();
        if line.starts_with('@') && line_start < before.len() {
            end = line_start;
            continue;
        }
        return None;
    }
}

/// What a documented item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    /// `class`
    Class,
    /// `struct`
    Struct,
    /// `interface`
    Interface,
    /// `record`
    Record,
    /// `enum`
    Enum,
    /// `annotation`
    Annotation,
    /// `type Name = ...;`
    TypeAlias,
    /// A top-level function.
    Function,
    /// A top-level `const`.
    Constant,
    /// A constructor.
    Constructor,
    /// A method.
    Method,
    /// A field.
    Field,
    /// A property.
    Property,
    /// An operator.
    Operator,
    /// An enum case.
    Variant,
    /// A function of an `unsafe native` block.
    Foreign,
}

impl ItemKind {
    /// The word the pages print for this kind.
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Class => "class",
            ItemKind::Struct => "struct",
            ItemKind::Interface => "interface",
            ItemKind::Record => "record",
            ItemKind::Enum => "enum",
            ItemKind::Annotation => "annotation",
            ItemKind::TypeAlias => "type alias",
            ItemKind::Function => "function",
            ItemKind::Constant => "constant",
            ItemKind::Constructor => "constructor",
            ItemKind::Method => "method",
            ItemKind::Field => "field",
            ItemKind::Property => "property",
            ItemKind::Operator => "operator",
            ItemKind::Variant => "case",
            ItemKind::Foreign => "foreign function",
        }
    }
}

/// One documented declaration.
#[derive(Debug, Clone)]
pub struct DocItem {
    /// What it is.
    pub kind: ItemKind,
    /// Its simple name.
    pub name: String,
    /// The declaration's header as written, whitespace collapsed.
    pub signature: String,
    /// Its doc comment, parsed (empty when there is none).
    pub doc: DocComment,
    /// Where it is declared.
    pub file: PathBuf,
    /// Line of the declaration, 1-based.
    pub line: u32,
    /// Members, for a type.
    pub members: Vec<DocItem>,
}

/// Every public item of one Jux package.
#[derive(Debug, Clone, Default)]
pub struct DocPackage {
    /// Dotted package name, empty for package-less files.
    pub name: String,
    /// Items in declaration order within each file, files in path order.
    pub items: Vec<DocItem>,
}

/// Collect the public API of `sources`, grouped by Jux package. Files that do
/// not parse are skipped (the build reports their errors); a file that parses
/// with errors still contributes what it has.
pub fn collect(sources: &[SourceFile]) -> Vec<DocPackage> {
    let mut packages: BTreeMap<String, DocPackage> = BTreeMap::new();
    for source in sources {
        let lexed = juxc_lex::lex(source);
        let parsed = juxc_parse::parse(&lexed.tokens);
        let unit = parsed.ast;
        let pkg_name = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
            .unwrap_or_default();
        let ctx = FileCtx { source };
        let entry = packages
            .entry(pkg_name.clone())
            .or_insert_with(|| DocPackage { name: pkg_name.clone(), items: Vec::new() });
        for decl in &unit.items {
            entry.items.extend(ctx.top_level(decl));
        }
    }
    packages.into_values().filter(|p| !p.items.is_empty()).collect()
}

/// Reads comments and signatures out of one file.
struct FileCtx<'a> {
    source: &'a SourceFile,
}

impl FileCtx<'_> {
    fn text(&self) -> &str {
        self.source.contents()
    }

    /// Build a [`DocItem`] for the declaration whose span starts at `start`.
    fn item(&self, kind: ItemKind, name: &str, start: u32, members: Vec<DocItem>) -> DocItem {
        let start = with_modifiers(self.text(), start as usize);
        let doc = doc_comment_before(self.text(), start)
            .map(|raw| parse_doc_comment(&raw))
            .unwrap_or_default();
        let (line, _) = self.source.line_col(start.min(self.text().len()));
        DocItem {
            kind,
            name: name.to_string(),
            signature: signature_at(self.text(), start, kind),
            doc,
            file: self.source.path().to_path_buf(),
            line,
            members,
        }
    }

    fn top_level(&self, decl: &TopLevelDecl) -> Vec<DocItem> {
        match decl {
            TopLevelDecl::Function(f) if visible(f.visibility, false) => {
                vec![self.item(ItemKind::Function, &f.name.text, f.span.start, Vec::new())]
            }
            TopLevelDecl::Const(c) if visible(c.visibility, false) => {
                vec![self.item(ItemKind::Constant, &c.name.text, c.span.start, Vec::new())]
            }
            TopLevelDecl::TypeAlias(t) if visible(t.visibility, false) => {
                vec![self.item(ItemKind::TypeAlias, &t.name.text, t.span.start, Vec::new())]
            }
            TopLevelDecl::Annotation(a) if visible(a.visibility, false) => {
                vec![self.item(ItemKind::Annotation, &a.name.text, a.span.start, Vec::new())]
            }
            TopLevelDecl::Class(c) if visible(c.visibility, false) => {
                let mut members = Vec::new();
                for ctor in &c.constructors {
                    if visible(ctor.visibility, false) {
                        members.push(self.item(ItemKind::Constructor, &c.name.text, ctor.span.start, Vec::new()));
                    }
                }
                self.fields(&c.fields, false, &mut members);
                for p in &c.properties {
                    if visible(p.visibility, false) {
                        members.push(self.item(ItemKind::Property, &p.name.text, p.span.start, Vec::new()));
                    }
                }
                self.methods(&c.methods, false, &mut members);
                self.operators(&c.operators, &mut members);
                let mut items = vec![self.item(
                    if c.is_struct { ItemKind::Struct } else { ItemKind::Class },
                    &c.name.text,
                    c.span.start,
                    members,
                )];
                for nested in &c.nested_types {
                    items.extend(self.top_level(nested));
                }
                items
            }
            TopLevelDecl::Interface(i) if visible(i.visibility, false) => {
                let mut members = Vec::new();
                self.fields(&i.fields, true, &mut members);
                for p in &i.properties {
                    if visible(p.visibility, true) {
                        members.push(self.item(ItemKind::Property, &p.name.text, p.span.start, Vec::new()));
                    }
                }
                self.methods(&i.methods, true, &mut members);
                self.operators(&i.operators, &mut members);
                vec![self.item(ItemKind::Interface, &i.name.text, i.span.start, members)]
            }
            TopLevelDecl::Record(r) if visible(r.visibility, false) => {
                let mut members = Vec::new();
                for ctor in r.compact_ctor.iter().chain(&r.constructors) {
                    if visible(ctor.visibility, true) {
                        members.push(self.item(ItemKind::Constructor, &r.name.text, ctor.span.start, Vec::new()));
                    }
                }
                self.fields(&r.static_fields, false, &mut members);
                self.methods(&r.methods, false, &mut members);
                self.operators(&r.operators, &mut members);
                vec![self.item(ItemKind::Record, &r.name.text, r.span.start, members)]
            }
            TopLevelDecl::Enum(e) if visible(e.visibility, false) => {
                let mut members = Vec::new();
                for v in &e.variants {
                    members.push(self.item(ItemKind::Variant, &v.name.text, v.span.start, Vec::new()));
                }
                self.fields(&e.constants, true, &mut members);
                self.fields(&e.fields, false, &mut members);
                self.methods(&e.methods, false, &mut members);
                self.operators(&e.operators, &mut members);
                vec![self.item(ItemKind::Enum, &e.name.text, e.span.start, members)]
            }
            TopLevelDecl::ExternBlock(b) => b
                .fns
                .iter()
                .filter(|f| visible(f.visibility, true))
                .map(|f| self.item(ItemKind::Foreign, &f.name.text, f.span.start, Vec::new()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn fields(&self, fields: &[juxc_ast::FieldDecl], implicit_public: bool, out: &mut Vec<DocItem>) {
        for f in fields {
            // A property's synthesized backing field is not API.
            if f.origin_property.is_some() || !visible(f.visibility, implicit_public) {
                continue;
            }
            out.push(self.item(ItemKind::Field, &f.name.text, f.span.start, Vec::new()));
        }
    }

    fn methods(&self, methods: &[juxc_ast::FnDecl], implicit_public: bool, out: &mut Vec<DocItem>) {
        for m in methods {
            if m.is_property || !visible(m.visibility, implicit_public) {
                continue;
            }
            out.push(self.item(ItemKind::Method, &m.name.text, m.span.start, Vec::new()));
        }
    }

    fn operators(&self, ops: &[juxc_ast::OperatorDecl], out: &mut Vec<DocItem>) {
        for o in ops {
            if o.is_deleted || !visible(o.visibility, true) {
                continue;
            }
            let sig = signature_at(self.text(), o.span.start as usize, ItemKind::Operator);
            let name = sig
                .split_once("operator")
                .map(|(_, rest)| format!("operator{}", rest.split('(').next().unwrap_or("").trim_end()))
                .unwrap_or_else(|| "operator".to_string());
            out.push(self.item(ItemKind::Operator, &name, o.span.start, Vec::new()));
        }
    }
}

/// Declaration spans start at the declaration's own keyword or type, after
/// its modifiers. Step back over the modifier words in front of `start` so
/// the signature shows them and the doc comment is found in front of them.
fn with_modifiers(text: &str, start: usize) -> usize {
    const MODIFIERS: &[&str] = &[
        "public", "protected", "private", "internal", "static", "final", "abstract", "sealed",
        "non-sealed", "async", "unsafe", "const", "weak", "ref", "override", "default", "open",
        "volatile", "native",
    ];
    let mut at = start.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    loop {
        let before = text[..at].trim_end();
        let word_start = before
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .map(|p| p + 1)
            .unwrap_or(0);
        let word = &before[word_start..];
        if word.is_empty() || !MODIFIERS.contains(&word) {
            return at;
        }
        at = word_start;
    }
}

/// Is a declaration with this visibility part of the documented API? Public
/// and protected are; so is an unmarked member where members are public by
/// default (interfaces, enum constants, operators).
fn visible(v: Visibility, implicit_public: bool) -> bool {
    match v {
        Visibility::Public | Visibility::Protected => true,
        Visibility::Package => implicit_public,
        Visibility::Internal | Visibility::Private => false,
    }
}

/// The declaration header starting at `start`: up to the body's `{`, the
/// terminating `;`, an initializer's `=` (fields and constants keep their
/// value only when it is a constant), or an expression body's `->`.
/// Annotation arguments and generic brackets are skipped over, and runs of
/// whitespace become one space.
fn signature_at(text: &str, start: usize, kind: ItemKind) -> String {
    let rest = &text[start.min(text.len())..];
    let bytes = rest.as_bytes();
    let mut depth_paren = 0i32;
    let mut end = rest.len();
    let mut i = 0usize;
    let mut in_str = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'{' | b';' if depth_paren == 0 => {
                end = i;
                break;
            }
            b'=' if depth_paren == 0 => {
                let next = bytes.get(i + 1).copied();
                let prev = if i > 0 { bytes[i - 1] } else { b' ' };
                // `==`, `<=`, `>=`, `!=` inside an operator's name are not
                // initializers.
                let is_op_char = matches!(next, Some(b'=')) || matches!(prev, b'=' | b'<' | b'>' | b'!');
                if !is_op_char && kind != ItemKind::Constant && kind != ItemKind::Operator {
                    end = i;
                    break;
                }
                if !is_op_char && kind == ItemKind::Constant {
                    // A constant shows its value: keep scanning to the `;`.
                }
            }
            b'-' if depth_paren == 0 && bytes.get(i + 1) == Some(&b'>') && kind != ItemKind::Operator => {
                end = i;
                break;
            }
            _ => {}
        }
        i += 1;
    }
    rest[..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// HTML output
// ---------------------------------------------------------------------------

/// Escape text for HTML.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The page file for a Jux package.
fn page_file(pkg: &str) -> String {
    if pkg.is_empty() {
        "root.html".to_string()
    } else {
        format!("{pkg}.html")
    }
}

/// Where every documented type lives: simple name → `page#anchor`.
fn link_table(packages: &[DocPackage]) -> BTreeMap<String, String> {
    let mut links = BTreeMap::new();
    for p in packages {
        for item in &p.items {
            let target = format!("{}#{}", page_file(&p.name), item.name);
            links.entry(item.name.clone()).or_insert(target.clone());
            if !p.name.is_empty() {
                links.insert(format!("{}.{}", p.name, item.name), target);
            }
        }
    }
    links
}

/// Escape `text` and turn every word naming a documented item into a link.
fn linkify(text: &str, links: &BTreeMap<String, String>, skip: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        match links.get(word.as_str()) {
            Some(href) if word != skip => {
                let _ = write!(out, "<a href=\"{}\">{}</a>", esc(href), esc(word));
            }
            _ => out.push_str(&esc(word)),
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' || (c == '.' && !word.is_empty()) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push_str(&esc(&c.to_string()));
        }
    }
    flush(&mut word, &mut out);
    out
}

/// A deliberately small Markdown renderer: headings, paragraphs, bullet and
/// numbered lists, fenced code, inline code, bold, italic and links. Doc
/// comments need no more, and a dependency-free renderer keeps the output
/// the same on every machine.
pub fn markdown_to_html(md: &str, links: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut para: Vec<String> = Vec::new();
    let mut list: Option<(&'static str, Vec<String>)> = None;
    let mut fence: Option<(String, Vec<String>)> = None;

    fn flush_para(para: &mut Vec<String>, out: &mut String, links: &BTreeMap<String, String>) {
        if !para.is_empty() {
            let _ = writeln!(out, "<p>{}</p>", inline(&para.join(" "), links));
            para.clear();
        }
    }
    fn flush_list(list: &mut Option<(&'static str, Vec<String>)>, out: &mut String, links: &BTreeMap<String, String>) {
        if let Some((tag, items)) = list.take() {
            let _ = write!(out, "<{tag}>");
            for i in items {
                let _ = write!(out, "<li>{}</li>", inline(&i, links));
            }
            let _ = writeln!(out, "</{tag}>");
        }
    }

    for line in md.lines() {
        let trimmed = line.trim();
        if let Some((info, lines)) = fence.as_mut() {
            if trimmed.starts_with("```") {
                let lang = info.split_whitespace().next().unwrap_or("");
                let _ = writeln!(
                    out,
                    "<pre class=\"code\" data-lang=\"{}\"><code>{}</code></pre>",
                    esc(lang),
                    esc(&lines.join("\n")),
                );
                fence = None;
            } else {
                lines.push(line.to_string());
            }
            continue;
        }
        if let Some(info) = trimmed.strip_prefix("```") {
            flush_para(&mut para, &mut out, links);
            flush_list(&mut list, &mut out, links);
            fence = Some((info.trim().to_string(), Vec::new()));
            continue;
        }
        if trimmed.is_empty() {
            flush_para(&mut para, &mut out, links);
            flush_list(&mut list, &mut out, links);
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes > 0 && hashes <= 6 && trimmed[hashes..].starts_with(' ') {
            flush_para(&mut para, &mut out, links);
            flush_list(&mut list, &mut out, links);
            // Headings inside an item's docs sit below the item's own heading.
            let level = (hashes + 3).min(6);
            let _ = writeln!(out, "<h{level}>{}</h{level}>", inline(trimmed[hashes..].trim(), links));
            continue;
        }
        let bullet = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* "));
        let numbered = trimmed
            .split_once(". ")
            .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            .map(|(_, rest)| rest);
        if let Some(text) = bullet.or(numbered) {
            flush_para(&mut para, &mut out, links);
            let tag = if bullet.is_some() { "ul" } else { "ol" };
            match list.as_mut() {
                Some((t, items)) if *t == tag => items.push(text.to_string()),
                _ => {
                    flush_list(&mut list, &mut out, links);
                    list = Some((tag, vec![text.to_string()]));
                }
            }
            continue;
        }
        if let Some((_, items)) = list.as_mut() {
            // A continuation line of the last list item.
            if line.starts_with("  ") {
                if let Some(last) = items.last_mut() {
                    last.push(' ');
                    last.push_str(trimmed);
                    continue;
                }
            }
            flush_list(&mut list, &mut out, links);
        }
        para.push(trimmed.to_string());
    }
    if let Some((info, lines)) = fence {
        let lang = info.split_whitespace().next().unwrap_or("");
        let _ = writeln!(out, "<pre class=\"code\" data-lang=\"{}\"><code>{}</code></pre>", esc(lang), esc(&lines.join("\n")));
    }
    flush_para(&mut para, &mut out, links);
    flush_list(&mut list, &mut out, links);
    out
}

/// Inline Markdown: `code`, **bold**, *italic*, [text](url). Code spans that
/// name a documented item link to it.
fn inline(text: &str, links: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == '`') {
                let code: String = chars[i + 1..i + 1 + close].iter().collect();
                match links.get(code.as_str()) {
                    Some(href) => {
                        let _ = write!(out, "<a href=\"{}\"><code>{}</code></a>", esc(href), esc(&code));
                    }
                    None => {
                        let _ = write!(out, "<code>{}</code>", esc(&code));
                    }
                }
                i += close + 2;
                continue;
            }
        }
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(close) = find_seq(&chars, i + 2, &['*', '*']) {
                let inner: String = chars[i + 2..close].iter().collect();
                let _ = write!(out, "<strong>{}</strong>", inline(&inner, links));
                i = close + 2;
                continue;
            }
        }
        if (c == '*' || c == '_') && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == c) {
                let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                if !inner.is_empty() && !inner.ends_with(' ') {
                    let _ = write!(out, "<em>{}</em>", inline(&inner, links));
                    i += close + 2;
                    continue;
                }
            }
        }
        if c == '[' {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == ']') {
                let label_end = i + 1 + close;
                if chars.get(label_end + 1) == Some(&'(') {
                    if let Some(paren) = chars[label_end + 2..].iter().position(|&x| x == ')') {
                        let label: String = chars[i + 1..label_end].iter().collect();
                        let url: String = chars[label_end + 2..label_end + 2 + paren].iter().collect();
                        let _ = write!(out, "<a href=\"{}\">{}</a>", esc(&url), inline(&label, links));
                        i = label_end + 3 + paren;
                        continue;
                    }
                }
            }
        }
        out.push_str(&esc(&c.to_string()));
        i += 1;
    }
    out
}

fn find_seq(chars: &[char], from: usize, seq: &[char]) -> Option<usize> {
    (from..chars.len().saturating_sub(seq.len() - 1)).find(|&k| chars[k..].starts_with(seq))
}

const STYLE: &str = "\
:root{--bg:#fff;--fg:#1f2328;--muted:#59636e;--line:#d1d9e0;--code:#f6f8fa;--link:#0969da;--warn:#9a6700}\
@media (prefers-color-scheme:dark){:root{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--line:#3d444d;--code:#151b23;--link:#4493f8;--warn:#d29922}}\
*{box-sizing:border-box}body{margin:0;font:15px/1.55 system-ui,-apple-system,'Segoe UI',sans-serif;background:var(--bg);color:var(--fg)}\
header{border-bottom:1px solid var(--line);padding:12px 24px;display:flex;gap:16px;align-items:center;flex-wrap:wrap}\
header a.home{font-weight:600;color:var(--fg);text-decoration:none}main{max-width:980px;margin:0 auto;padding:16px 24px 64px}\
a{color:var(--link)}code,pre{font-family:ui-monospace,Consolas,monospace;font-size:13px}\
pre.code,pre.sig{background:var(--code);border:1px solid var(--line);border-radius:6px;padding:10px 12px;overflow-x:auto}\
pre.sig{white-space:pre-wrap}p code{background:var(--code);padding:1px 4px;border-radius:4px}\
.item{border-top:1px solid var(--line);padding-top:8px;margin-top:24px}.member{margin:14px 0 0 16px}\
.kind{color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.04em}.muted{color:var(--muted)}\
.deprecated{color:var(--warn);font-weight:600}dl.tags dt{font-weight:600;margin-top:6px}dl.tags dd{margin-left:16px}\
#search{padding:6px 10px;border:1px solid var(--line);border-radius:6px;background:var(--bg);color:var(--fg);min-width:260px}\
#results{list-style:none;padding:0}#results li{padding:4px 0;border-bottom:1px solid var(--line)}\
table{border-collapse:collapse}td,th{border:1px solid var(--line);padding:4px 8px;text-align:left}";

const SEARCH_JS: &str = "\
(function(){var box=document.getElementById('search');if(!box)return;var list=document.getElementById('results');\
function run(){var q=box.value.trim().toLowerCase();list.innerHTML='';if(!q){return;}\
var hits=JUX_SEARCH.filter(function(e){return e.n.toLowerCase().indexOf(q)>=0||e.s.toLowerCase().indexOf(q)>=0;}).slice(0,50);\
hits.forEach(function(e){var li=document.createElement('li');var a=document.createElement('a');a.href=e.u;a.textContent=e.n;\
var k=document.createElement('span');k.className='kind';k.textContent=' '+e.k+' ';var s=document.createElement('span');s.className='muted';s.textContent=e.s;\
li.appendChild(a);li.appendChild(k);li.appendChild(s);list.appendChild(li);});}box.addEventListener('input',run);run();})();";

/// Where one package's docs were written.
#[derive(Debug, Clone)]
pub struct DocOutput {
    /// `target/doc/index.html`.
    pub index: PathBuf,
    /// How many items were documented.
    pub items: usize,
}

/// Write the documentation site for `manifest`'s package into `out_dir`.
pub fn write_site(manifest: &Manifest, packages: &[DocPackage], out_dir: &Path) -> Result<DocOutput> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let links = link_table(packages);
    let title = &manifest.package.name;
    let version = manifest.package.version.as_deref().unwrap_or("");

    // ---- one page per Jux package ----
    let mut search: Vec<String> = Vec::new();
    let mut count = 0usize;
    for p in packages {
        let page = page_file(&p.name);
        let display = if p.name.is_empty() { "(root package)".to_string() } else { p.name.clone() };
        let mut body = String::new();
        let _ = writeln!(body, "<h1><span class=\"kind\">package</span> {}</h1>", esc(&display));
        for item in &p.items {
            count += 1 + item.members.len();
            render_item(&mut body, item, &links, &item.name, 2);
            search.push(search_entry(&item.name, item.kind, &format!("{page}#{}", item.name), &item.doc));
            for m in &item.members {
                let anchor = format!("{}.{}", item.name, m.name);
                search.push(search_entry(
                    &format!("{}.{}", item.name, m.name),
                    m.kind,
                    &format!("{page}#{anchor}"),
                    &m.doc,
                ));
            }
        }
        write_page(out_dir, &page, &format!("{display} - {title}"), title, &body)?;
    }

    // ---- index: overview, package list, dependencies (§B.14.8), search ----
    let mut body = String::new();
    let _ = writeln!(body, "<h1>{} <span class=\"muted\">{}</span></h1>", esc(title), esc(version));
    if let Some(desc) = manifest.package.description.as_deref() {
        let _ = writeln!(body, "<p>{}</p>", esc(desc));
    }
    body.push_str("<h2>Packages</h2>\n<ul>\n");
    for p in packages {
        let display = if p.name.is_empty() { "(root package)" } else { p.name.as_str() };
        let _ = writeln!(
            body,
            "<li><a href=\"{}\">{}</a> <span class=\"muted\">{} items</span></li>",
            esc(&page_file(&p.name)),
            esc(display),
            p.items.len(),
        );
    }
    body.push_str("</ul>\n");
    body.push_str(&dependencies_section(manifest));
    body.push_str("<h2>Search</h2>\n<ul id=\"results\"></ul>\n");
    write_page(out_dir, "index.html", title, title, &body)?;

    let js = format!("var JUX_SEARCH=[{}];\n{SEARCH_JS}\n", search.join(","));
    std::fs::write(out_dir.join("search.js"), js).context("writing search.js")?;
    std::fs::write(out_dir.join("style.css"), STYLE).context("writing style.css")?;
    Ok(DocOutput { index: out_dir.join("index.html"), items: count })
}

fn search_entry(name: &str, kind: ItemKind, url: &str, doc: &DocComment) -> String {
    format!(
        "{{\"n\":{},\"k\":{},\"u\":{},\"s\":{}}}",
        json_str(name),
        json_str(kind.label()),
        json_str(url),
        json_str(&doc.summary()),
    )
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

fn write_page(out_dir: &Path, file: &str, page_title: &str, site: &str, body: &str) -> Result<()> {
    let html = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n\
         <header><a class=\"home\" href=\"index.html\">{}</a>\
         <input id=\"search\" type=\"search\" placeholder=\"Search the API\" aria-label=\"Search the API\"></header>\n\
         <main>\n{}{}</main>\n<script src=\"search.js\"></script>\n</body>\n</html>\n",
        esc(page_title),
        esc(site),
        if file == "index.html" { "" } else { "<ul id=\"results\"></ul>\n" },
        body,
    );
    std::fs::write(out_dir.join(file), html).with_context(|| format!("writing {file}"))
}

/// One item (and, for a type, its members) as HTML.
fn render_item(out: &mut String, item: &DocItem, links: &BTreeMap<String, String>, anchor: &str, level: u8) {
    let class = if level == 2 { "item" } else { "member" };
    let _ = writeln!(
        out,
        "<section class=\"{class}\" id=\"{}\">\n<h{level}><span class=\"kind\">{}</span> {}</h{level}>",
        esc(anchor),
        esc(item.kind.label()),
        esc(&item.name),
    );
    let _ = writeln!(out, "<pre class=\"sig\"><code>{}</code></pre>", linkify(&item.signature, links, &item.name));
    if let Some(reason) = &item.doc.deprecated {
        let _ = writeln!(out, "<p class=\"deprecated\">Deprecated{}</p>", if reason.is_empty() { String::new() } else { format!(": {}", inline(reason, links)) });
    }
    out.push_str(&markdown_to_html(&item.doc.body, links));
    let d = &item.doc;
    if !d.params.is_empty() || d.returns.is_some() || !d.throws.is_empty() || d.since.is_some() || !d.see.is_empty() {
        out.push_str("<dl class=\"tags\">\n");
        if !d.params.is_empty() {
            out.push_str("<dt>Parameters</dt>\n");
            for (name, text) in &d.params {
                let _ = writeln!(out, "<dd><code>{}</code> {}</dd>", esc(name), inline(text, links));
            }
        }
        if let Some(r) = &d.returns {
            let _ = writeln!(out, "<dt>Returns</dt>\n<dd>{}</dd>", inline(r, links));
        }
        if !d.throws.is_empty() {
            out.push_str("<dt>Throws</dt>\n");
            for (ty, text) in &d.throws {
                let _ = writeln!(out, "<dd><code>{}</code> {}</dd>", linkify(ty, links, ""), inline(text, links));
            }
        }
        if let Some(s) = &d.since {
            let _ = writeln!(out, "<dt>Since</dt>\n<dd>{}</dd>", esc(s));
        }
        if !d.see.is_empty() {
            out.push_str("<dt>See also</dt>\n");
            for s in &d.see {
                let _ = writeln!(out, "<dd>{}</dd>", linkify(s, links, ""));
            }
        }
        out.push_str("</dl>\n");
    }
    let _ = writeln!(
        out,
        "<p class=\"muted\">{}:{}</p>",
        esc(item.file.file_name().and_then(|n| n.to_str()).unwrap_or("")),
        item.line,
    );
    for m in &item.members {
        render_item(out, m, links, &format!("{}.{}", item.name, m.name), (level + 1).min(4));
    }
    out.push_str("</section>\n");
}

/// The `## Dependencies` section of §B.14.8: native libraries from
/// `[ffi.*]`, Jux packages, and Rust crates.
fn dependencies_section(manifest: &Manifest) -> String {
    let mut native = Vec::new();
    for f in &manifest.ffi {
        let linkage = f.kind.as_deref().unwrap_or("dynamic");
        let place = match f.search_paths.first() {
            Some(p) => format!("{linkage}, from {p}"),
            None => format!("{linkage}, system"),
        };
        let mut libs = vec![f.lib.clone()];
        libs.extend(f.extra_libs.iter().cloned());
        native.push(format!("<li><strong>{}</strong> ({})</li>", esc(&libs.join(", ")), esc(&place)));
    }
    let mut jux = Vec::new();
    let mut rust = Vec::new();
    for d in &manifest.dependencies {
        let version = d
            .version
            .as_deref()
            .map(|v| format!(" v{v}"))
            .or_else(|| d.git.as_deref().map(|g| format!(" ({g})")))
            .or_else(|| d.path.as_ref().map(|_| " (path)".to_string()))
            .unwrap_or_default();
        let line = format!("<li>{}{}</li>", esc(&d.name), esc(&version));
        if d.name.starts_with("rust.") {
            rust.push(line);
        } else if !d.name.starts_with("c.") && !d.name.starts_with("cpp.") {
            jux.push(line);
        }
    }
    if native.is_empty() && jux.is_empty() && rust.is_empty() {
        return String::new();
    }
    let mut out = String::from("<h2>Dependencies</h2>\n");
    for (title, list) in [("Native Libraries", native), ("Jux Packages", jux), ("Rust Crates", rust)] {
        if !list.is_empty() {
            let _ = writeln!(out, "<h3>{title}</h3>\n<ul>\n{}\n</ul>", list.join("\n"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Doctests
// ---------------------------------------------------------------------------

/// One runnable doc example, ready to compile.
#[derive(Debug, Clone)]
pub struct Doctest {
    /// `Package.Item` (or `Item.member`) the example documents.
    pub owner: String,
    /// Where the item is declared, for the report.
    pub location: String,
    /// The complete program text.
    pub program: String,
    /// Compile only.
    pub no_run: bool,
}

/// Every ```` ```jux ```` example in `packages`, turned into a program: an
/// example that declares its own `main` is used as written; otherwise its
/// `import` lines are kept at the top, the rest becomes the body of `main`,
/// and the documented item's package is imported with `.*` so the example can
/// name the item the way the docs do.
pub fn doctests(packages: &[DocPackage]) -> Vec<Doctest> {
    let mut out = Vec::new();
    for p in packages {
        for item in &p.items {
            collect_doctests(p, item, &item.name, &mut out);
            for m in &item.members {
                collect_doctests(p, m, &format!("{}.{}", item.name, m.name), &mut out);
            }
        }
    }
    out
}

fn collect_doctests(p: &DocPackage, item: &DocItem, owner: &str, out: &mut Vec<Doctest>) {
    for ex in item.doc.examples() {
        if ex.ignore {
            continue;
        }
        let owner = if p.name.is_empty() { owner.to_string() } else { format!("{}.{owner}", p.name) };
        out.push(Doctest {
            owner,
            location: format!("{}:{}", item.file.display(), item.line),
            program: doctest_program(&p.name, &ex.code),
            no_run: ex.no_run,
        });
    }
}

/// Wrap an example's code into a whole program (see [`doctests`]).
pub fn doctest_program(package: &str, code: &str) -> String {
    let declares_main = code.contains("void main(") || code.contains("int main(");
    let mut imports: Vec<String> = Vec::new();
    if !package.is_empty() {
        imports.push(format!("import {package}.*;"));
    }
    let mut body: Vec<&str> = Vec::new();
    for line in code.lines() {
        let t = line.trim_start();
        if t.starts_with("import ") && body.iter().all(|l| l.trim().is_empty()) {
            imports.push(t.to_string());
        } else {
            body.push(line);
        }
    }
    let mut program = imports.join("\n");
    program.push_str("\n\n");
    if declares_main {
        program.push_str(&body.join("\n"));
        program.push('\n');
    } else {
        program.push_str("public void main() {\n");
        for line in body {
            program.push_str("    ");
            program.push_str(line);
            program.push('\n');
        }
        program.push_str("}\n");
    }
    program
}

/// The outcome of one doctest.
#[derive(Debug, Clone)]
pub struct DoctestResult {
    /// The item it documents.
    pub owner: String,
    /// Where that item is declared.
    pub location: String,
    /// `None` on success, else what went wrong.
    pub failure: Option<String>,
}

/// Compile (and unless `no_run`, run) every doctest of `manifest`'s package
/// against its library code, one program per example, under
/// `<emit_root>/doctest-<n>/`. Returns one result per doctest.
pub fn run_doctests(
    manifest: &Manifest,
    dep_sources: &[SourceFile],
    tests: &[Doctest],
    emit_root: &Path,
    release: bool,
    cfg: &crate::cfg::CfgFacts,
) -> Result<Vec<DoctestResult>> {
    let mut results = Vec::new();
    let staging = emit_root.join("doctest-src");
    for (n, test) in tests.iter().enumerate() {
        let dir = staging.join(format!("doctest_{n}"));
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let file = dir.join("main.jux");
        std::fs::write(&file, &test.program).with_context(|| format!("writing {}", file.display()))?;
        let example = crate::project::Example { name: format!("doctest_{n}"), files: vec![file] };
        let failure = match crate::project::build_example(manifest, dep_sources, emit_root, release, &example, cfg) {
            Err(e) => Some(format!("{e:#}")),
            Ok(build) if build.has_errors() => Some(render_diagnostics(&build)),
            Ok(build) => match (test.no_run, build.binaries.first()) {
                (true, _) => None,
                (false, None) => Some("the example produced no program".to_string()),
                (false, Some(bin)) => {
                    let output = std::process::Command::new(&bin.binary_path)
                        .output()
                        .with_context(|| format!("running {}", bin.binary_path.display()))?;
                    if output.status.success() {
                        None
                    } else {
                        Some(format!(
                            "exited with {}\n{}{}",
                            output.status,
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr),
                        ))
                    }
                }
            },
        };
        results.push(DoctestResult { owner: test.owner.clone(), location: test.location.clone(), failure });
    }
    Ok(results)
}

fn render_diagnostics(build: &crate::project::PackageBuild) -> String {
    let mut out = String::new();
    for d in &build.diagnostics {
        if !matches!(d.severity, juxc_diagnostics::Severity::Error) {
            continue;
        }
        let place = match (d.file, d.primary_span) {
            (Some(i), Some(span)) if i < build.sources.len() => {
                let (line, col) = build.sources[i].line_col(span.start as usize);
                format!("{}:{line}:{col}: ", build.sources[i].path().display())
            }
            _ => String::new(),
        };
        let _ = writeln!(out, "{place}[{}] {}", d.code, d.message);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "/**\n * Adds two numbers.\n *\n * More text here.\n *\n * @param a the first\n * @param b the second,\n *   continued\n * @return the sum\n * @throws Overflow when it does\n * @since 0.2\n * @see Calculator\n *\n * ```jux\n * print(add(1, 2));\n * ```\n */";

    #[test]
    fn tags_are_pulled_out_of_the_body() {
        let d = parse_doc_comment(SAMPLE);
        assert_eq!(d.summary(), "Adds two numbers.");
        assert_eq!(d.params, vec![("a".into(), "the first".into()), ("b".into(), "the second, continued".into())]);
        assert_eq!(d.returns.as_deref(), Some("the sum"));
        assert_eq!(d.throws, vec![("Overflow".into(), "when it does".into())]);
        assert_eq!(d.since.as_deref(), Some("0.2"));
        assert_eq!(d.see, vec!["Calculator".to_string()]);
        let ex = d.examples();
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].code.trim(), "print(add(1, 2));");
    }

    #[test]
    fn a_comment_is_found_over_annotations() {
        let text = "/** Old. */\n@Deprecated\npublic void f() {}\n";
        let start = text.find("public").unwrap();
        assert_eq!(doc_comment_before(text, start).as_deref(), Some("/** Old. */"));
        let plain = "/* not a doc */\npublic void g() {}\n";
        assert_eq!(doc_comment_before(plain, plain.find("public").unwrap()), None);
    }

    #[test]
    fn signatures_stop_at_the_body() {
        let text = "public int add(int a, int b) { return a + b; }";
        assert_eq!(signature_at(text, 0, ItemKind::Function), "public int add(int a, int b)");
        let field = "public   int count = 3;";
        assert_eq!(signature_at(field, 0, ItemKind::Field), "public int count");
        let konst = "public const int MAX = 10;";
        assert_eq!(signature_at(konst, 0, ItemKind::Constant), "public const int MAX = 10");
        let op = "public bool operator==(Point other) { return true; }";
        assert_eq!(signature_at(op, 0, ItemKind::Operator), "public bool operator==(Point other)");
        let arrow = "public int twice(int x) -> x * 2;";
        assert_eq!(signature_at(arrow, 0, ItemKind::Function), "public int twice(int x)");
    }

    #[test]
    fn doctest_programs_wrap_statements() {
        let p = doctest_program("geo", "import rust.std.Vec;\nvar v = new Vec<int>();\nprint(v.len());");
        assert!(p.starts_with("import geo.*;\nimport rust.std.Vec;"), "{p}");
        assert!(p.contains("public void main() {\n    var v = new Vec<int>();"), "{p}");
        let whole = doctest_program("", "public void main() { print(1); }");
        assert!(!whole.contains("public void main() {\n    public"), "{whole}");
    }

    #[test]
    fn markdown_renders_the_basics() {
        let links = BTreeMap::from([("Point".to_string(), "geo.html#Point".to_string())]);
        let html = markdown_to_html("A **bold** `Point` and *more*.\n\n- one\n- two\n\n```jux\nx < y\n```", &links);
        assert!(html.contains("<strong>bold</strong>"), "{html}");
        assert!(html.contains("<a href=\"geo.html#Point\"><code>Point</code></a>"), "{html}");
        assert!(html.contains("<ul><li>one</li><li>two</li></ul>"), "{html}");
        assert!(html.contains("x &lt; y"), "{html}");
    }
}
