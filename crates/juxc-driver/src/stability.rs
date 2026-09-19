//! Leaving nightly-only APIs out of the generated `rust.std` surface
//! (JUX-BINDGEN-ADDENDUM §G.6.2.3).
//!
//! The std surface is read from the `rust-docs-json` component, which exists
//! only for the nightly toolchain, so it describes nightly's std: unstable
//! methods such as `Path::is_empty` or `<[f64]>::sort_floats` appear next to
//! the stable ones. A program builds with the user's own toolchain, where
//! calling one is rustc E0658. rustdoc JSON does not record stability, so the
//! answer is asked of the compiler that will build the program: every item and
//! method of the surface is named once in a probe crate, the probe is checked
//! by the default (stable) `rustc`, and whatever it rejects with E0658 is left
//! out of the stub.
//!
//! The probe only ever REMOVES what rustc reports as unstable. A probe line
//! that fails for another reason (a wrong argument count, a bound the unit
//! type does not meet) says nothing about stability and keeps its item.

use std::collections::{HashMap, HashSet};
use std::process::Command;

use juxc_bindgen::model::{StubItem, TypeKind};
use juxc_bindgen::StubFile;

/// What one probe line names.
#[derive(Clone, Copy)]
enum Probe {
    /// The whole item `items[i]`.
    Item(usize),
    /// Method `j` of the type `items[i]`; the probe's body starts at the given
    /// column. Only an E0658 inside the body is about the method: one in the
    /// signature is about the type arguments the probe had to write
    /// (`Vec<(), ()>` names the unstable allocator parameter).
    Method(usize, usize, usize),
}

/// Remove from `stub` every item and method the default `rustc` rejects as
/// unstable (E0658). A no-op when `rustc` cannot be run.
pub(crate) fn prune_unstable(stub: &mut StubFile) {
    let (source, probes) = probe_source(stub);
    let Some(unstable) = run_probe(&source) else {
        return;
    };
    let mut drop_items: HashSet<usize> = HashSet::new();
    let mut drop_methods: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (line, column) in unstable {
        match probes.get(&line) {
            Some(Probe::Item(i)) => {
                drop_items.insert(*i);
            }
            Some(Probe::Method(i, j, body)) if column >= *body => {
                drop_methods.entry(*i).or_default().insert(*j);
            }
            _ => {}
        }
    }
    for (i, methods) in drop_methods {
        if let Some(StubItem::Type(t)) = stub.items.get_mut(i) {
            let mut j = 0usize;
            t.methods.retain(|_| {
                let keep = !methods.contains(&j);
                j += 1;
                keep
            });
        }
    }
    let mut i = 0usize;
    stub.items.retain(|_| {
        let keep = !drop_items.contains(&i);
        i += 1;
        keep
    });
}

/// The probe crate, and which line names what.
fn probe_source(stub: &StubFile) -> (String, HashMap<usize, Probe>) {
    let mut out = String::from("#![allow(unused, deprecated, unused_must_use, invalid_value)]\n");
    let mut probes: HashMap<usize, Probe> = HashMap::new();
    let mut line = 2usize;
    let mut push = |out: &mut String, text: String, probe: Probe| {
        out.push_str(&text);
        out.push('\n');
        probes.insert(line, probe);
        line += 1;
    };
    for (i, item) in stub.items.iter().enumerate() {
        match item {
            StubItem::Type(t) => {
                let Some(path) = t.rust_path.as_deref() else { continue };
                let primitive = t.primitive.is_some();
                if !primitive && path.contains("::") {
                    push(&mut out, format!("use {path} as __jux_item_{i};"), Probe::Item(i));
                }
                if t.kind == TypeKind::Interface {
                    continue;
                }
                // A method may exist only for some instantiations
                // (`sort_floats` is `[f64]`'s, `isolate_highest_one` needs an
                // integer), so a generic type is probed with a few type
                // arguments; the method is unstable if any probe says so.
                let instantiations: Vec<String> = if t.generics.is_empty() {
                    vec![String::new()]
                } else {
                    // Every parameter set to the candidate, and (for a type
                    // with defaulted trailing parameters, `Vec<T, A = Global>`)
                    // only the first one.
                    let mut forms = Vec::new();
                    for a in ["()", "f64", "u32"] {
                        forms.push(format!("<{}>", vec![a; t.generics.len()].join(", ")));
                        if t.generics.len() > 1 {
                            forms.push(format!("<{a}>"));
                        }
                    }
                    forms
                };
                for (j, m) in t.methods.iter().enumerate() {
                    if is_rust_keyword(&m.name) {
                        continue;
                    }
                    for (k, args) in instantiations.iter().enumerate() {
                        // A static is named without type arguments: written
                        // out they would sit in the body, and `Vec::<(), ()>`
                        // names the unstable allocator parameter itself.
                        let body = if m.is_static {
                            format!("let _ = {path}::{};", m.name)
                        } else {
                            format!("let _ = x.{}();", m.name)
                        };
                        let head = format!("fn __jux_m_{i}_{j}_{k}(x: &mut {path}{args}) {{ ");
                        // rustc columns are 1-based.
                        let body_column = head.chars().count() + 1;
                        push(&mut out, format!("{head}{body} }}"), Probe::Method(i, j, body_column));
                    }
                }
            }
            StubItem::Function(f) => {
                if let Some(path) = f.rust_path.as_deref().filter(|p| p.contains("::")) {
                    push(&mut out, format!("use {path} as __jux_item_{i};"), Probe::Item(i));
                }
            }
            StubItem::Const(c) => {
                if let Some(path) = c.rust_path.as_deref().filter(|p| p.contains("::")) {
                    push(&mut out, format!("use {path} as __jux_item_{i};"), Probe::Item(i));
                }
            }
            StubItem::Alias(_) => {}
        }
    }
    (out, probes)
}

/// Check the probe with the default `rustc` and return the (line, column)
/// positions it reports E0658 at. `None` when `rustc` could not be run at all.
fn run_probe(source: &str) -> Option<HashSet<(usize, usize)>> {
    let dir = std::env::temp_dir().join(format!("juxc-stability-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let src = dir.join("probe.rs");
    std::fs::write(&src, source).ok()?;
    let output = Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg("--crate-type")
        .arg("lib")
        .arg("--emit=metadata")
        .arg("--error-format=json")
        .arg("--cap-lints")
        .arg("allow")
        .arg("-o")
        .arg(dir.join("probe.rmeta"))
        .arg(&src)
        .output()
        .ok()?;
    let _ = std::fs::remove_dir_all(&dir);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut found = HashSet::new();
    for msg in stderr.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(msg) else { continue };
        if v.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()) != Some("E0658") {
            continue;
        }
        let Some(spans) = v.get("spans").and_then(|s| s.as_array()) else { continue };
        for span in spans {
            if span.get("is_primary").and_then(|p| p.as_bool()) != Some(true) {
                continue;
            }
            let line = span.get("line_start").and_then(|l| l.as_u64());
            let column = span.get("column_start").and_then(|c| c.as_u64());
            if let (Some(l), Some(c)) = (line, column) {
                found.insert((l as usize, c as usize));
            }
        }
    }
    Some(found)
}

/// Whether `name` is a Rust keyword, which a probe would have to raw-escape.
fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false"
            | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move"
            | "mut" | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct"
            | "super" | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while"
            | "async" | "await" | "dyn" | "abstract" | "become" | "box" | "do" | "final"
            | "macro" | "override" | "priv" | "typeof" | "unsized" | "virtual" | "yield"
            | "try" | "gen"
    )
}
