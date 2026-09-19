//! Conditional compilation: `@cfg(...)` and `if cfg(...)` (JUX-LANG-V1 §11,
//! JUX-COMPILER-PIPELINE-ADDENDUM §C.2.5).
//!
//! The predicates are decided here, against the facts of the build being
//! made, right after parsing and before any name is resolved. A declaration
//! whose predicate is false is removed from the tree; an `if cfg` statement is
//! replaced by the branch it selects. Nothing later in the pipeline sees
//! either form, which is what lets the code for another platform call APIs
//! this one does not have: it is never resolved or type-checked, only parsed.
//!
//! The facts:
//!
//! | key             | value                                                     |
//! |-----------------|-----------------------------------------------------------|
//! | `os`            | `windows`, `linux`, `macos`, `freebsd`, `none`, ...       |
//! | `arch`          | `x86_64`, `aarch64`, `x86`, `armv7`, `armv6m`, `riscv32`, ... |
//! | `endian`        | `little` / `big`                                          |
//! | `pointer_width` | `16` / `32` / `64`                                        |
//! | `target`        | the full target triple                                    |
//! | `profile`       | `full` / `embedded` / `core` (`[build] profile`)          |
//! | `feature`       | a feature enabled for the package the file belongs to     |
//! | `debug`         | flag: the build is not optimized                          |
//! | `release`       | flag: the build is optimized                              |
//!
//! The target is the cross-compilation triple when one was requested
//! (`--target`, `[build] target`) and the host otherwise.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use juxc_ast::{
    Annotation, AnnotationArg, Block, ClassDecl, CompilationUnit, ElseBranch, Expr, FnDecl,
    InterpSegment, LambdaBody, Literal, Stmt, SwitchBody, TopLevelDecl,
};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::{SourceFile, Span};

/// The keys a `key = "value"` predicate may name.
const KEYS: &[&str] = &["os", "arch", "endian", "pointer_width", "target", "profile", "feature"];

/// The flags a bare predicate may name.
const FLAGS: &[&str] = &["debug", "release"];

/// What a build is, as `@cfg` sees it.
#[derive(Debug, Clone)]
pub struct CfgFacts {
    os: String,
    arch: String,
    endian: String,
    pointer_width: String,
    /// The requested triple; `None` builds for the host, whose triple is only
    /// looked up when a predicate asks for `target`.
    target: Option<String>,
    release: bool,
    profile: juxc_tycheck::Profile,
    /// The enabled features of each package, by the package's root directory.
    /// A file takes the set of the deepest root that contains it.
    features: Vec<(PathBuf, BTreeSet<String>)>,
}

impl Default for CfgFacts {
    /// A debug build of the full profile for the build's target, with no
    /// features: what a loose file and the editor see.
    fn default() -> Self {
        CfgFacts::new(false, juxc_tycheck::Profile::Full)
    }
}

impl CfgFacts {
    /// The facts for a build of `profile`, optimized when `release`, for the
    /// cross-compilation target when one was requested and the host otherwise.
    pub fn new(release: bool, profile: juxc_tycheck::Profile) -> Self {
        let mut facts = match crate::cross_target() {
            Some(triple) => CfgFacts::for_triple(&triple),
            None => CfgFacts::host(),
        };
        facts.release = release;
        facts.profile = profile;
        facts
    }

    /// The host this compiler runs on.
    fn host() -> Self {
        CfgFacts {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            endian: if cfg!(target_endian = "big") { "big" } else { "little" }.to_string(),
            pointer_width: (usize::BITS).to_string(),
            target: None,
            release: false,
            profile: juxc_tycheck::Profile::Full,
            features: Vec::new(),
        }
    }

    /// The facts a target triple implies: `thumbv7em-none-eabihf` is `arch =
    /// "armv7"`, `os = "none"`, 32-bit, little-endian.
    pub fn for_triple(triple: &str) -> Self {
        let first = triple.split('-').next().unwrap_or(triple);
        let arch = if first.starts_with("thumbv6m") {
            "armv6m".to_string()
        } else if first.starts_with("thumbv7") || first.starts_with("armv7") {
            "armv7".to_string()
        } else if first.starts_with("riscv32") {
            "riscv32".to_string()
        } else if first.starts_with("riscv64") {
            "riscv64".to_string()
        } else if matches!(first, "i386" | "i586" | "i686") {
            "x86".to_string()
        } else {
            first.to_string()
        };
        let os = if triple.contains("windows") {
            "windows"
        } else if triple.contains("android") {
            "android"
        } else if triple.contains("linux") {
            "linux"
        } else if triple.contains("darwin") {
            "macos"
        } else if triple.contains("-ios") {
            "ios"
        } else if triple.contains("freebsd") {
            "freebsd"
        } else if triple.contains("netbsd") {
            "netbsd"
        } else if triple.contains("openbsd") {
            "openbsd"
        } else if triple.contains("wasi") {
            "wasi"
        } else if triple.contains("-none") || triple.contains("-unknown-unknown") {
            "none"
        } else {
            "unknown"
        }
        .to_string();
        let pointer_width = if matches!(first, "avr" | "msp430") {
            "16"
        } else if first.ends_with("64") || first.starts_with("aarch64") || first.starts_with("riscv64")
            || first.starts_with("powerpc64") || first.starts_with("mips64") || first == "s390x"
            || first.starts_with("sparcv9") || first.starts_with("loongarch64")
        {
            "64"
        } else {
            "32"
        }
        .to_string();
        let big = first == "s390x"
            || first.starts_with("sparc")
            || first == "m68k"
            || first.ends_with("eb")
            || first == "aarch64_be"
            || (first.starts_with("powerpc") && !first.ends_with("le"))
            || (first.starts_with("mips") && !first.ends_with("el"));
        CfgFacts {
            os,
            arch,
            endian: if big { "big" } else { "little" }.to_string(),
            pointer_width,
            target: Some(triple.to_string()),
            release: false,
            profile: juxc_tycheck::Profile::Full,
            features: Vec::new(),
        }
    }

    /// The same facts for a package of another language profile.
    pub fn with_profile(mut self, profile: juxc_tycheck::Profile) -> Self {
        self.profile = profile;
        self
    }

    /// Enable `features` for the package rooted at `root`.
    pub fn with_package_features(mut self, root: PathBuf, features: BTreeSet<String>) -> Self {
        self.features.push((root, features));
        self
    }

    /// The features enabled for the file at `path`.
    fn features_for(&self, path: &Path) -> Option<&BTreeSet<String>> {
        let path = lexically_normal(path);
        self.features
            .iter()
            .map(|(root, set)| (lexically_normal(root), set))
            .filter(|(root, _)| path.starts_with(root))
            .max_by_key(|(root, _)| root.components().count())
            .map(|(_, set)| set)
    }

    /// The target triple, asking `rustc` for the host's when none was given.
    fn triple(&self) -> String {
        if let Some(t) = &self.target {
            return t.clone();
        }
        static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        HOST.get_or_init(|| {
            std::process::Command::new("rustc")
                .arg("-vV")
                .output()
                .ok()
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
                })
                .unwrap_or_default()
        })
        .clone()
    }

    /// The language profile the build is for.
    pub fn profile(&self) -> juxc_tycheck::Profile {
        self.profile
    }

    fn profile_name(&self) -> &'static str {
        match self.profile {
            juxc_tycheck::Profile::Full => "full",
            juxc_tycheck::Profile::Embedded => "embedded",
            juxc_tycheck::Profile::Core => "core",
        }
    }
}

/// `path` with `.` and `dir/..` pairs removed, without touching the disk. A
/// path dependency's root is written relative to its dependent
/// (`app/../core`), and a file under it has to match either spelling.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if out.file_name().is_some() => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// A predicate that could not be decided: what is wrong, and where.
struct Invalid(String, Span);

/// Decide `@cfg` and `if cfg` across `units`, in place. `sources[i]` is the
/// file `units[i]` was parsed from; declaration stubs (`.jux.d`) are left
/// alone. Every problem with a predicate is an `E0150` in `diagnostics`, and
/// the code it guards is then kept, so the rest of it is still checked.
pub fn apply(
    units: &mut [CompilationUnit],
    sources: &[SourceFile],
    facts: &CfgFacts,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (idx, unit) in units.iter_mut().enumerate() {
        let Some(source) = sources.get(idx) else { continue };
        if crate::stubs::is_stub_path(source.path()) {
            continue;
        }
        let mut pass = Pass {
            facts,
            features: facts.features_for(source.path()),
            diagnostics: Vec::new(),
        };
        pass.unit(unit);
        for mut d in pass.diagnostics {
            d.file = Some(idx);
            diagnostics.push(d);
        }
    }
}

/// Remove what the build excludes from `unit`, without reporting anything:
/// for the registry's discovery parse, which the real pass repeats with
/// diagnostics. Invalid predicates keep their code, as there.
pub(crate) fn apply_quiet(unit: &mut CompilationUnit, facts: &CfgFacts, path: &Path) {
    let mut pass = Pass { facts, features: facts.features_for(path), diagnostics: Vec::new() };
    pass.unit(unit);
}

struct Pass<'a> {
    facts: &'a CfgFacts,
    features: Option<&'a BTreeSet<String>>,
    diagnostics: Vec<Diagnostic>,
}

impl Pass<'_> {
    /// Whether every `@cfg` in `annotations` holds.
    fn keep(&mut self, annotations: &[Annotation]) -> bool {
        annotations
            .iter()
            .filter(|a| {
                a.name.segments.len() == 1 && a.name.segments[0].text.eq_ignore_ascii_case("cfg")
            })
            .all(|a| {
                if a.args.is_empty() {
                    self.invalid(Invalid(
                        "`@cfg()` has no predicate; write one, such as `@cfg(os = \"linux\")`".into(),
                        a.span,
                    ));
                    return true;
                }
                self.holds(&a.args)
            })
    }

    /// Whether all of `predicate` holds, reporting what cannot be decided.
    fn holds(&mut self, predicate: &[AnnotationArg]) -> bool {
        match predicate.iter().map(|p| self.eval_arg(p)).collect::<Result<Vec<bool>, Invalid>>() {
            Ok(values) => values.into_iter().all(|v| v),
            Err(e) => {
                self.invalid(e);
                true
            }
        }
    }

    fn invalid(&mut self, Invalid(message, span): Invalid) {
        self.diagnostics.push(
            Diagnostic::error(code::Code::E0150_InvalidCfgPredicate, message).with_span(span),
        );
    }

    fn eval_arg(&self, arg: &AnnotationArg) -> Result<bool, Invalid> {
        match arg {
            AnnotationArg::Named { name, value } => self.leaf(&name.text, Some(value), name.span),
            AnnotationArg::Positional(e) => self.eval_expr(e),
        }
    }

    /// A predicate in expression form: a flag, or `all` / `any` / `not`.
    fn eval_expr(&self, e: &Expr) -> Result<bool, Invalid> {
        match e {
            // The parser's stand-in for a predicate it could not read, and
            // already reported. It holds, so the code it guards is checked.
            Expr::Literal(Literal::Null) => Ok(true),
            Expr::Path(qn) if qn.segments.len() == 1 => self.leaf(&qn.segments[0].text, None, qn.span),
            Expr::Call(c) => {
                let Expr::Path(qn) = &*c.callee else {
                    return Err(Invalid("expected `all(...)`, `any(...)` or `not(...)`".into(), c.span));
                };
                let combinator = qn.segments.last().map(|s| s.text.to_ascii_lowercase()).unwrap_or_default();
                let parts: Vec<bool> = c
                    .args
                    .iter()
                    .zip(c.arg_names.iter())
                    .map(|(value, name)| match name {
                        Some(key) => self.leaf(&key.text, Some(value), key.span),
                        None => self.eval_expr(value),
                    })
                    .collect::<Result<_, _>>()?;
                match combinator.as_str() {
                    "all" => Ok(parts.into_iter().all(|v| v)),
                    "any" => Ok(parts.into_iter().any(|v| v)),
                    "not" if parts.len() == 1 => Ok(!parts[0]),
                    "not" => Err(Invalid(
                        format!("`not(...)` takes exactly one predicate, and this one has {}", parts.len()),
                        c.span,
                    )),
                    other => Err(Invalid(
                        format!("`{other}(...)` is not a cfg combinator; the combinators are `all`, `any` and `not`"),
                        qn.span,
                    )),
                }
            }
            other => Err(Invalid(
                "expected a cfg predicate: `key = \"value\"`, a flag such as `debug`, or `all(...)`, \
                 `any(...)`, `not(...)`"
                    .into(),
                other.span(),
            )),
        }
    }

    /// `key = value`, or the bare flag `key` when `value` is `None`.
    fn leaf(&self, key: &str, value: Option<&Expr>, span: Span) -> Result<bool, Invalid> {
        let key = key.to_ascii_lowercase();
        let text = match value {
            None => None,
            Some(Expr::Literal(Literal::String(s))) => Some(s.clone()),
            Some(Expr::Literal(Literal::Bool(b))) => Some(b.to_string()),
            Some(Expr::Literal(Literal::Int(n))) => Some(n.value.to_string()),
            Some(_) => {
                return Err(Invalid(format!("the value of `{key}` must be a literal, such as \"linux\""), span));
            }
        };
        if FLAGS.contains(&key.as_str()) {
            let set = if key == "release" { self.facts.release } else { !self.facts.release };
            return match text.as_deref() {
                None | Some("true") => Ok(set),
                Some("false") => Ok(!set),
                Some(other) => Err(Invalid(
                    format!("`{key}` is a flag: write `{key}`, or `{key} = \"true\"`, not `\"{other}\"`"),
                    span,
                )),
            };
        }
        if !KEYS.contains(&key.as_str()) {
            return Err(Invalid(
                format!(
                    "`{key}` is not a cfg key; the keys are {} and the flags are {}",
                    KEYS.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", "),
                    FLAGS.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", "),
                ),
                span,
            ));
        }
        let Some(want) = text else {
            return Err(Invalid(format!("`{key}` needs a value, such as `{key} = \"...\"`"), span));
        };
        let is = |have: &str| have.eq_ignore_ascii_case(&want);
        Ok(match key.as_str() {
            "os" => is(&self.facts.os),
            "arch" => is(&self.facts.arch),
            "endian" => is(&self.facts.endian),
            "pointer_width" => is(&self.facts.pointer_width),
            "target" => is(&self.facts.triple()),
            "profile" => is(self.facts.profile_name()),
            "feature" => self.features.is_some_and(|set| set.contains(&want)),
            _ => unreachable!("every key in KEYS is matched"),
        })
    }

    // ---- the tree ---------------------------------------------------------

    fn unit(&mut self, unit: &mut CompilationUnit) {
        let imports = std::mem::take(&mut unit.imports);
        unit.imports = imports
            .into_iter()
            .filter_map(|mut import| {
                let keep = match import.cfg.take() {
                    Some(cfg) => self.keep(std::slice::from_ref(&cfg)),
                    None => true,
                };
                keep.then_some(import)
            })
            .collect();
        self.items(&mut unit.items);
    }

    fn items(&mut self, items: &mut Vec<TopLevelDecl>) {
        items.retain(|item| self.keep(item_annotations(item)));
        for item in items {
            self.item(item);
        }
    }

    fn item(&mut self, item: &mut TopLevelDecl) {
        match item {
            TopLevelDecl::Function(f) => self.function(f),
            TopLevelDecl::Class(c) => self.class(c),
            TopLevelDecl::Interface(i) => {
                i.methods.retain(|m| self.keep(&m.annotations));
                i.fields.retain(|f| self.keep(&f.annotations));
                for m in &mut i.methods {
                    self.function(m);
                }
                for f in &mut i.fields {
                    if let Some(e) = &mut f.default {
                        self.expr(e);
                    }
                }
            }
            TopLevelDecl::Record(r) => {
                r.methods.retain(|m| self.keep(&m.annotations));
                r.static_fields.retain(|f| self.keep(&f.annotations));
                for m in &mut r.methods {
                    self.function(m);
                }
                for op in &mut r.operators {
                    if let Some(b) = &mut op.body {
                        self.block(b);
                    }
                }
                for f in &mut r.static_fields {
                    if let Some(e) = &mut f.default {
                        self.expr(e);
                    }
                }
            }
            TopLevelDecl::Enum(e) => {
                e.methods.retain(|m| self.keep(&m.annotations));
                e.constants.retain(|c| self.keep(&c.annotations));
                for m in &mut e.methods {
                    self.function(m);
                }
                for op in &mut e.operators {
                    if let Some(b) = &mut op.body {
                        self.block(b);
                    }
                }
                for c in &mut e.constants {
                    if let Some(init) = &mut c.default {
                        self.expr(init);
                    }
                }
            }
            TopLevelDecl::Const(c) => self.expr(&mut c.value),
            TopLevelDecl::Annotation(_) | TopLevelDecl::TypeAlias(_) | TopLevelDecl::ExternBlock(_) => {}
        }
    }

    fn class(&mut self, c: &mut ClassDecl) {
        // A property was desugared at parse time into a backing field, a
        // getter and a setter, which carry no annotations of their own. When
        // the property goes, they go with it.
        let mut dropped_props: Vec<String> = Vec::new();
        let props = std::mem::take(&mut c.properties);
        c.properties = props
            .into_iter()
            .filter(|p| {
                let keep = self.keep(&p.annotations);
                if !keep {
                    dropped_props.push(p.name.text.clone());
                }
                keep
            })
            .collect();
        c.fields.retain(|f| {
            !dropped_props.iter().any(|p| f.name.text == juxc_ast::desugar_backing_field_name(p))
                && self.keep(&f.annotations)
        });
        c.methods.retain(|m| {
            !dropped_props
                .iter()
                .any(|p| m.name.text == *p || m.name.text == juxc_ast::desugar_static_setter_name(p))
                && self.keep(&m.annotations)
        });
        c.constructors.retain(|k| self.keep(&k.annotations));
        self.items(&mut c.nested_types);
        for f in &mut c.fields {
            if let Some(e) = &mut f.default {
                self.expr(e);
            }
        }
        for k in &mut c.constructors {
            for p in &mut k.params {
                if let Some(d) = &mut p.default {
                    self.expr(d);
                }
            }
            self.block(&mut k.body);
        }
        for m in &mut c.methods {
            self.function(m);
        }
        for op in &mut c.operators {
            if let Some(b) = &mut op.body {
                self.block(b);
            }
        }
        for b in c
            .init_blocks
            .iter_mut()
            .chain(c.static_init_blocks.iter_mut())
            .chain(c.drop_blocks.iter_mut())
        {
            self.block(b);
        }
        for p in &mut c.properties {
            let bodies = p
                .getter
                .as_mut()
                .map(|g| &mut g.body)
                .into_iter()
                .chain(p.setter.as_mut().map(|s| &mut s.body));
            for body in bodies.collect::<Vec<_>>() {
                match body {
                    juxc_ast::AccessorBody::Block(b) => self.block(b),
                    juxc_ast::AccessorBody::Expr(e) => self.expr(e),
                    juxc_ast::AccessorBody::Auto => {}
                }
            }
            if let Some(init) = &mut p.initializer {
                self.expr(init);
            }
        }
    }

    fn function(&mut self, f: &mut FnDecl) {
        for p in &mut f.params {
            if let Some(d) = &mut p.default {
                self.expr(d);
            }
        }
        if let Some(body) = &mut f.body {
            self.block(body);
        }
    }

    fn block(&mut self, b: &mut Block) {
        let statements = std::mem::take(&mut b.statements);
        for stmt in statements {
            match stmt {
                // The statement becomes the branch it selects, as a block so
                // its declarations stay scoped to it, or nothing at all.
                Stmt::IfCfg(c) => {
                    let chosen = if self.holds(&c.predicate) { Some(c.then_block) } else { c.else_block };
                    if let Some(mut chosen) = chosen {
                        self.block(&mut chosen);
                        b.statements.push(Stmt::Block(chosen));
                    }
                }
                mut other => {
                    self.stmt(&mut other);
                    b.statements.push(other);
                }
            }
        }
    }

    fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Expr(e) | Stmt::Throw(e, _) | Stmt::Yield(e, _) => self.expr(e),
            Stmt::Return(e, _) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            Stmt::VarDecl(v) => {
                if let Some(init) = &mut v.init {
                    self.expr(init);
                }
            }
            Stmt::If(i) => self.if_stmt(i),
            Stmt::While(w) => {
                self.expr(&mut w.condition);
                self.block(&mut w.body);
            }
            Stmt::DoWhile(d) => {
                self.block(&mut d.body);
                self.expr(&mut d.condition);
            }
            Stmt::ForEach(f) => {
                self.expr(&mut f.iter);
                self.block(&mut f.body);
            }
            Stmt::ForC(f) => {
                if let Some(init) = &mut f.init {
                    self.stmt(init);
                }
                if let Some(cond) = &mut f.cond {
                    self.expr(cond);
                }
                if let Some(update) = &mut f.update {
                    self.stmt(update);
                }
                self.block(&mut f.body);
            }
            Stmt::Assign(a) => {
                self.expr(&mut a.target);
                self.expr(&mut a.value);
            }
            Stmt::Labeled { stmt, .. } => self.stmt(stmt),
            Stmt::SuperCall(args, _) => {
                for a in args {
                    self.expr(a);
                }
            }
            Stmt::Try(t) => {
                self.block(&mut t.body);
                for c in &mut t.catches {
                    self.block(&mut c.body);
                }
                if let Some(f) = &mut t.finally {
                    self.block(f);
                }
            }
            Stmt::Block(b) | Stmt::Unsafe(b) => self.block(b),
            // Only reachable nested in a position `block` does not rebuild;
            // a statement slot always sits in a block, so there is none.
            Stmt::IfCfg(c) => {
                self.block(&mut c.then_block);
                if let Some(b) = &mut c.else_block {
                    self.block(b);
                }
            }
            Stmt::Break(..) | Stmt::Continue(..) => {}
        }
    }

    fn if_stmt(&mut self, i: &mut juxc_ast::IfStmt) {
        self.expr(&mut i.condition);
        self.block(&mut i.then_block);
        if let Some(else_branch) = &mut i.else_branch {
            match &mut **else_branch {
                ElseBranch::If(elif) => self.if_stmt(elif),
                ElseBranch::Block(b) => self.block(b),
            }
        }
    }

    /// Expressions only matter for the blocks inside them: lambdas, anonymous
    /// classes, `switch` arms and `try` expressions.
    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Lambda(l) => match &mut l.body {
                LambdaBody::Expr(b) => self.expr(b),
                LambdaBody::Block(b) => self.block(b),
            },
            Expr::NewObject(n) => {
                for a in &mut n.args {
                    self.expr(a);
                }
                if let Some(body) = &mut n.anonymous_body {
                    body.methods.retain(|m| self.keep(&m.annotations));
                    for m in &mut body.methods {
                        self.function(m);
                    }
                    for b in &mut body.init_blocks {
                        self.block(b);
                    }
                }
            }
            Expr::Switch(s) => {
                self.expr(&mut s.scrutinee);
                for arm in &mut s.arms {
                    if let Some(g) = &mut arm.guard {
                        self.expr(g);
                    }
                    match &mut arm.body {
                        SwitchBody::Expr(b) => self.expr(b),
                        SwitchBody::Block(b) => self.block(b),
                    }
                }
            }
            Expr::TryExpr(t) => {
                self.block(&mut t.body);
                for c in &mut t.catches {
                    self.block(&mut c.body);
                }
                if let Some(f) = &mut t.finally {
                    self.block(f);
                }
            }
            Expr::Call(c) => {
                self.expr(&mut c.callee);
                for a in &mut c.args {
                    self.expr(a);
                }
            }
            Expr::Binary(b) => {
                self.expr(&mut b.left);
                self.expr(&mut b.right);
            }
            Expr::Unary(u) => self.expr(&mut u.operand),
            Expr::Ternary(t) => {
                self.expr(&mut t.condition);
                self.expr(&mut t.then_branch);
                self.expr(&mut t.else_branch);
            }
            Expr::Elvis(el) => {
                self.expr(&mut el.value);
                self.expr(&mut el.fallback);
            }
            Expr::Field(f) => self.expr(&mut f.object),
            Expr::Index(i) => {
                self.expr(&mut i.array);
                self.expr(&mut i.index);
            }
            Expr::Cast(c) => self.expr(&mut c.value),
            Expr::TypeTest(t) => self.expr(&mut t.value),
            Expr::Await(inner, _)
            | Expr::NotNullAssert(inner, _)
            | Expr::ErrorProp(inner, _)
            | Expr::TypeOf(inner, _)
            | Expr::Out(inner, _) => self.expr(inner),
            Expr::TupleLit(items, _) => {
                for item in items {
                    self.expr(item);
                }
            }
            Expr::NewArrayLit(n) => {
                for item in &mut n.elements {
                    self.expr(item);
                }
            }
            Expr::InterpString(s) => {
                for seg in &mut s.segments {
                    if let InterpSegment::Expr(inner) = seg {
                        self.expr(inner);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The annotations written on a top-level (or nested) declaration.
fn item_annotations(item: &TopLevelDecl) -> &[Annotation] {
    match item {
        TopLevelDecl::Function(f) => &f.annotations,
        TopLevelDecl::Class(c) => &c.annotations,
        TopLevelDecl::Enum(e) => &e.annotations,
        TopLevelDecl::Record(r) => &r.annotations,
        TopLevelDecl::Interface(i) => &i.annotations,
        TopLevelDecl::Annotation(a) => &a.annotations,
        TopLevelDecl::TypeAlias(t) => &t.annotations,
        TopLevelDecl::Const(c) => &c.annotations,
        TopLevelDecl::ExternBlock(e) => &e.annotations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(src: &str) -> CompilationUnit {
        let file = SourceFile::new(PathBuf::from("probe.jux"), src.to_string());
        juxc_parse::parse(&juxc_lex::lex(&file).tokens).ast
    }

    fn names(unit: &CompilationUnit) -> Vec<String> {
        unit.items
            .iter()
            .filter_map(|i| match i {
                TopLevelDecl::Function(f) => Some(f.name.text.clone()),
                TopLevelDecl::Class(c) => Some(c.name.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn triples_imply_their_facts() {
        let t = CfgFacts::for_triple("thumbv7em-none-eabihf");
        assert_eq!((t.os.as_str(), t.arch.as_str(), t.pointer_width.as_str()), ("none", "armv7", "32"));
        let t = CfgFacts::for_triple("aarch64-apple-darwin");
        assert_eq!((t.os.as_str(), t.arch.as_str(), t.pointer_width.as_str()), ("macos", "aarch64", "64"));
        let t = CfgFacts::for_triple("x86_64-pc-windows-msvc");
        assert_eq!((t.os.as_str(), t.endian.as_str()), ("windows", "little"));
        let t = CfgFacts::for_triple("powerpc-unknown-linux-gnu");
        assert_eq!((t.endian.as_str(), t.pointer_width.as_str()), ("big", "32"));
        assert_eq!(CfgFacts::for_triple("avr-unknown-gnu-atmega328").pointer_width, "16");
    }

    #[test]
    fn declarations_and_statements_follow_the_predicate() {
        let facts = CfgFacts::for_triple("x86_64-unknown-linux-gnu")
            .with_package_features(PathBuf::from(""), ["json".to_string()].into_iter().collect());
        let mut u = unit(
            r#"
@cfg(os = "linux") void onLinux() { }
@cfg(os = "windows") void onWindows() { }
@cfg(any(os = "windows", all(arch = "x86_64", not(endian = "big")))) void either() { }
@cfg(feature = "json") void json() { }
@cfg(feature = "yaml") void yaml() { }
@cfg(debug) void debugOnly() { }
@cfg(release) void releaseOnly() { }
void main() {
    if cfg(os = "windows") { print("w"); } else if cfg(pointer_width = "64") { print("64"); }
}
"#,
        );
        let mut diagnostics = Vec::new();
        let src = [SourceFile::new(PathBuf::from("probe.jux"), String::new())];
        apply(std::slice::from_mut(&mut u), &src, &facts, &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(names(&u), ["onLinux", "either", "json", "debugOnly", "main"]);
        let TopLevelDecl::Function(main) = u.items.last().unwrap() else { panic!() };
        let body = main.body.as_ref().unwrap();
        // `if cfg(windows)` fell through to its else, itself an `if cfg` that holds.
        let Stmt::Block(outer) = &body.statements[0] else { panic!("{body:?}") };
        assert!(matches!(&outer.statements[0], Stmt::Block(inner) if inner.statements.len() == 1));
    }

    #[test]
    fn a_malformed_predicate_is_reported_and_keeps_the_code() {
        let facts = CfgFacts::for_triple("x86_64-unknown-linux-gnu");
        let mut u = unit("@cfg(platform = \"linux\") void a() { }\n@cfg(not(debug, release)) void b() { }\n");
        let mut diagnostics = Vec::new();
        let src = [SourceFile::new(PathBuf::from("probe.jux"), String::new())];
        apply(std::slice::from_mut(&mut u), &src, &facts, &mut diagnostics);
        assert_eq!(names(&u), ["a", "b"]);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(diagnostics.iter().all(|d| d.code == code::Code::E0150_InvalidCfgPredicate));
    }
}
