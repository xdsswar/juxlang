//! Stub rendering — IR → `.jux.d` text (§G.2, §G.5).
//!
//! Every body is elided to `;` and every field omits its initializer, so the
//! output is a valid signature-only Jux declaration file the resolver ingests
//! as `external` (§G.9). The rendering aims to read like hand-written Jux.

use std::fmt::Write as _;

use crate::model::{
    StubConst, StubCtor, StubField, StubFile, StubFn, StubItem, StubType, StubVariant, TypeKind,
};

/// Render a whole stub file to `.jux.d` source text.
pub fn render(file: &StubFile) -> String {
    let mut out = String::new();

    for line in &file.header {
        let _ = writeln!(out, "// {line}");
    }
    if !file.header.is_empty() {
        out.push('\n');
    }

    if !file.package.is_empty() {
        let _ = writeln!(out, "package {};\n", file.package);
    }

    for (i, item) in file.items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match item {
            StubItem::Type(t) => render_type(&mut out, t),
            StubItem::Function(f) => {
                // `@rust("real::path")` records a free function's true Rust path
                // so the backend imports it as `use real::path as juxName;`,
                // bridging snake_case Rust to the camelCase Jux stub name.
                if let Some(path) = &f.rust_path {
                    let _ = writeln!(out, "@rust(\"{path}\")");
                }
                out.push_str(&render_fn(f, false));
                out.push('\n');
            }
            StubItem::Const(c) => render_const(&mut out, c),
        }
    }

    out
}

/// Render one type declaration.
fn render_type(out: &mut String, t: &StubType) {
    if let Some(doc) = &t.doc {
        let _ = writeln!(out, "/** {doc} */");
    }
    // `@rust("real::path")` records the true Rust path so the backend can lower a
    // reference to this external type to its real symbol (§G.9.2) instead of the
    // flat Jux `rust.std.X` spelling.
    if let Some(path) = &t.rust_path {
        let _ = writeln!(out, "@rust(\"{path}\")");
    }

    let keyword = match t.kind {
        TypeKind::Class => "class",
        TypeKind::Interface => "interface",
        TypeKind::Struct => "struct",
        TypeKind::Record => "record",
        TypeKind::Enum => "enum",
    };
    let generics = render_generics(&t.generics);

    // Records carry their components in the header; everything else uses a body.
    if t.kind == TypeKind::Record {
        let comps = t
            .fields
            .iter()
            .map(|fld| format!("{} {}", fld.ty, fld.name))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "public {keyword} {}{generics}({comps}) {{", t.name);
        // Records may still expose methods.
        for m in &t.methods {
            let _ = writeln!(out, "    {}", render_fn(m, t.kind == TypeKind::Interface));
        }
        out.push_str("}\n");
        return;
    }

    let _ = writeln!(out, "public {keyword} {}{generics} {{", t.name);

    if t.kind == TypeKind::Enum {
        render_variants(out, &t.variants);
        if !t.methods.is_empty() {
            out.push('\n');
        }
    } else {
        for fld in &t.fields {
            render_field(out, fld);
        }
        for ctor in &t.constructors {
            render_ctor(out, ctor);
        }
    }

    for m in &t.methods {
        let _ = writeln!(out, "    {}", render_fn(m, t.kind == TypeKind::Interface));
    }

    out.push_str("}\n");
}

/// Render enum variants on one line: `A, B(int), C = 3` — no `case` keyword.
fn render_variants(out: &mut String, variants: &[StubVariant]) {
    if variants.is_empty() {
        out.push_str("    // (variants not represented)\n");
        return;
    }
    let rendered: Vec<String> = variants
        .iter()
        .map(|v| {
            let mut s = v.name.clone();
            if !v.payload.is_empty() {
                let payload = v
                    .payload
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                s.push_str(&format!("({payload})"));
            }
            if let Some(d) = v.discriminant {
                s.push_str(&format!(" = {d}"));
            }
            s
        })
        .collect();
    let _ = writeln!(out, "    {}", rendered.join(", "));
}

fn render_field(out: &mut String, f: &StubField) {
    let _ = writeln!(out, "    {}{} {};", f.visibility.prefix(), f.ty, f.name);
}

fn render_ctor(out: &mut String, c: &StubCtor) {
    let _ = writeln!(out, "    {}{}({});", c.visibility.prefix(), c.name, render_params(&c.params));
}

/// Render a method or free function. `in_interface` suppresses the `static`
/// modifier: a Jux interface method that carries `static` (or `default`) must
/// have a body, which a signature-only stub never does — so inside an interface
/// every member is surfaced as a plain abstract signature.
fn render_fn(f: &StubFn, in_interface: bool) -> String {
    let mut s = String::new();
    // `&mut self` receiver → `@MutSelf` marker. The compiler reads this
    // off the stub's symbol table to DISCOVER receiver mutability from
    // the real library signature (no hardcoded method-name lists).
    if f.is_mut_self {
        s.push_str("@MutSelf ");
    }
    s.push_str(f.visibility.prefix());
    // `static` is valid on a *class* stub method (no body needed there), but on
    // an interface a bodyless `static` is `E0200`. A Rust trait's associated
    // function (no `self`) is surfaced as a plain interface signature instead.
    if f.is_static && !in_interface {
        s.push_str("static ");
    }
    // An `unsafe` Rust fn surfaces with the Jux `unsafe` modifier (§A.2.4), so
    // the parser records it and the type checker demands an `unsafe` context at
    // every call site. Sits after `static`, before the return type.
    if f.is_unsafe {
        s.push_str("unsafe ");
    }
    // NB: a Rust trait's *provided* method (`f.is_default`) is NOT rendered with
    // the Jux `default` keyword. A `.jux.d` stub is signature-only, and a Jux
    // `default` method requires a body (else E0200) — which a stub never has.
    // The `default`-ness isn't representable in a bodyless view and is moot
    // anyway (stubs aren't lowered, §G.9), so it's surfaced as a plain
    // signature. `is_default` stays on the IR for non-stub consumers.
    let _ = write!(s, "{} {}{}({})", f.ret, f.name, render_generics(&f.generics), render_params(&f.params));
    if let Some(err) = &f.throws {
        let _ = write!(s, " throws {err}");
    }
    s.push(';');
    s
}

fn render_const(out: &mut String, c: &StubConst) {
    match &c.value {
        Some(v) => {
            let _ = writeln!(out, "public const {} {} = {v};", c.ty, c.name);
        }
        None => {
            let _ = writeln!(out, "public const {} {};", c.ty, c.name);
        }
    }
}

/// `<A, B>` or empty.
fn render_generics(generics: &[String]) -> String {
    if generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generics.join(", "))
    }
}

/// `ty name, ty name` parameter list.
fn render_params(params: &[crate::model::StubParam]) -> String {
    params
        .iter()
        .map(|p| {
            // A `&`-prefixed type marks a borrowed parameter (§G.9.2): the Jux
            // type is unchanged (borrow vanishes, §G.3.4), but the parser reads
            // the `&` back into a per-parameter flag so codegen re-adds the
            // call-site borrow.
            let amp = if p.by_ref { "&" } else { "" };
            format!("{amp}{} {}", p.ty, p.name)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::ty::JuxType;

    fn param(name: &str, ty: JuxType) -> StubParam {
        StubParam { name: name.into(), ty, by_ref: false }
    }

    #[test]
    fn renders_hashmap_stub() {
        let mut hm = StubType::new(TypeKind::Class, "HashMap");
        hm.generics = vec!["K".into(), "V".into()];
        hm.constructors.push(StubCtor {
            visibility: Vis::Public,
            name: "HashMap".into(),
            params: vec![],
        });
        hm.methods.push(StubFn {
            visibility: Vis::Public,
            is_static: false,
            is_default: false,
            name: "insert".into(),
            generics: vec![],
            params: vec![param("key", JuxType::Param("K".into())), param("value", JuxType::Param("V".into()))],
            ret: JuxType::Void,
            throws: None,
            is_unsafe: false,
            is_mut_self: false,
            rust_path: None,
            doc: None,
        });
        hm.methods.push(StubFn {
            visibility: Vis::Public,
            is_static: false,
            is_default: false,
            name: "get".into(),
            generics: vec![],
            params: vec![param("key", JuxType::Param("K".into()))],
            ret: JuxType::nullable(JuxType::Param("V".into())),
            throws: None,
            is_unsafe: false,
            is_mut_self: false,
            rust_path: None,
            doc: None,
        });

        let file = StubFile {
            package: "rust.std.collections".into(),
            header: vec![],
            items: vec![StubItem::Type(hm)],
        };

        let out = render(&file);
        assert!(out.contains("package rust.std.collections;"));
        assert!(out.contains("public class HashMap<K, V> {"));
        assert!(out.contains("public HashMap();"));
        assert!(out.contains("public void insert(K key, V value);"));
        assert!(out.contains("public V? get(K key);"));
    }

    #[test]
    fn renders_throws_and_enum() {
        let f = StubFn {
            visibility: Vis::Public,
            is_static: false,
            is_default: false,
            name: "parse".into(),
            generics: vec![],
            params: vec![param("s", JuxType::String)],
            ret: JuxType::user("Config"),
            throws: Some(JuxType::user("ConfigError")),
            is_unsafe: false,
            is_mut_self: false,
            rust_path: None,
            doc: None,
        };
        assert_eq!(
            render_fn(&f, false),
            "public Config parse(String s) throws ConfigError;",
        );

        let mut e = StubType::new(TypeKind::Enum, "Color");
        e.variants = vec![
            StubVariant { name: "Red".into(), payload: vec![], discriminant: None },
            StubVariant { name: "Custom".into(), payload: vec![JuxType::Prim("int")], discriminant: None },
        ];
        let file = StubFile { items: vec![StubItem::Type(e)], ..Default::default() };
        let out = render(&file);
        assert!(out.contains("public enum Color {"));
        assert!(out.contains("Red, Custom(int)"));
        // No Swift-style `case` prefix.
        assert!(!out.contains("case "));
    }

    /// A free function with a real Rust path renders a `@rust("…")` annotation
    /// above it, so the backend can alias the snake_case name on import.
    #[test]
    fn renders_rust_path_on_free_fn() {
        let f = StubFn {
            visibility: Vis::Public,
            is_static: false,
            is_default: false,
            name: "parseDuration".into(),
            generics: vec![],
            params: vec![param("s", JuxType::String)],
            ret: JuxType::user("Duration"),
            throws: Some(JuxType::user("DurationError")),
            is_unsafe: false,
            is_mut_self: false,
            rust_path: Some("humantime::parse_duration".into()),
            doc: None,
        };
        let file = StubFile {
            items: vec![StubItem::Function(f)],
            ..Default::default()
        };
        let out = render(&file);
        assert!(out.contains("@rust(\"humantime::parse_duration\")"), "{out}");
        assert!(
            out.contains("public Duration parseDuration(String s) throws DurationError;"),
            "{out}",
        );
    }

    /// An `unsafe` Rust fn surfaces with the Jux `unsafe` modifier (§A.2.4),
    /// after `static`, before the return type.
    #[test]
    fn renders_unsafe_modifier() {
        let f = StubFn {
            visibility: Vis::Public,
            is_static: false,
            is_default: false,
            name: "getpid".into(),
            generics: vec![],
            params: vec![],
            ret: JuxType::Prim("i32"),
            throws: None,
            is_unsafe: true,
            is_mut_self: false,
            rust_path: None,
            doc: None,
        };
        assert_eq!(render_fn(&f, false), "public unsafe i32 getpid();");
    }
}
