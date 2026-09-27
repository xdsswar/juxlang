//! What a failed build of the emitted crate tells the user (ERRATA
//! E116).
//!
//! Once the front end has accepted a program, the rest of the build is cargo
//! and rustc. Everything that goes wrong there used to reach the user as
//! cargo's own text, behind "`cargo build` failed for the emitted Rust crate":
//! rustc's borrow errors about a crate the programmer never wrote, the whole
//! `link.exe` command line, a flood of `E0463` for a target that was never
//! installed. None of it was a [`Diagnostic`], so `--diagnostic-format json`
//! never saw it, and the exit status was 1 whether the compiler or the
//! program was at fault.
//!
//! Here cargo's JSON messages (`--message-format=json`) become Jux
//! diagnostics, carried out of the driver as a [`BuildFailure`] error:
//!
//! - rustc rejecting the emitted Rust is `E0900`, an internal compiler error:
//!   Jux has no borrow checker a program could fail (ERRATA E23), so every
//!   such rejection is a bug in the compiler. It is reported at the `.jux`
//!   line the `// JUX:` markers map it to, in Jux words, with rustc's code as
//!   a note, and exit status 101 like any ICE;
//! - a failed link is `E0906`, with the one line of the linker's output that
//!   says why;
//! - a target whose standard library is missing is `E0904`, normally caught
//!   before cargo runs at all (`preflight_target`).
//!
//! cargo's full report travels along as [`BuildFailure::detail`], printed
//! under `--verbose`.

use std::fmt;
use std::path::Path;

use juxc_diagnostics::{code::Code, Diagnostic};
use juxc_source::{SourceFile, Span};

use crate::source_map::SourceMap;

/// A build that failed after the front end accepted the program, as Jux
/// diagnostics. Returned inside the driver's `anyhow::Error`; a binary
/// downcasts to it, prints [`Self::diagnostics`] in the format the user asked
/// for, and exits with [`Self::exit_code`].
#[derive(Debug)]
pub struct BuildFailure {
    /// What went wrong, one diagnostic per distinct failure.
    pub diagnostics: Vec<Diagnostic>,
    /// The `.jux` files the diagnostics point into, indexed by their `file`.
    pub sources: Vec<SourceFile>,
    /// cargo's own report, shown under `--verbose`.
    pub detail: String,
    /// [`crate::ice::ICE_EXIT_CODE`] when the compiler is at fault, else 1.
    pub exit_code: u8,
}

impl fmt::Display for BuildFailure {
    /// The one-line format, for a caller that prints the error as it stands.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = crate::render::render_text(
            &self.diagnostics,
            &self.sources,
            crate::render::DiagnosticFormat::Line,
            false,
        );
        f.write_str(text.trim_end())
    }
}

impl std::error::Error for BuildFailure {}

impl BuildFailure {
    /// A failure with no source location: the target, the manifest.
    pub(crate) fn plain(diagnostic: Diagnostic) -> Self {
        BuildFailure { diagnostics: vec![diagnostic], sources: Vec::new(), detail: String::new(), exit_code: 1 }
    }
}

/// Turn cargo's JSON messages from a failed build into a [`BuildFailure`].
///
/// `emitted` is every emitted `.rs` file as `(crate-relative path, contents)`,
/// the text rustc compiled, from which the `// JUX:` markers are read.
/// `None` when cargo reported no compiler error at all (a registry that could
/// not be reached, a manifest cargo refused): that is not something to dress
/// up as a Jux diagnostic, and the caller passes cargo's text through.
pub fn from_cargo_messages(messages: &str, emitted: &[(&str, &str)]) -> Option<BuildFailure> {
    from_messages(messages, &SourceMap::from_sources(emitted))
}

/// [`from_cargo_messages`] against a map already built from disk.
pub(crate) fn from_messages(messages: &str, map: &SourceMap) -> Option<BuildFailure> {
    let mut failure = BuildFailure { diagnostics: Vec::new(), sources: Vec::new(), detail: String::new(), exit_code: 1 };
    let mut seen: Vec<(Code, Option<Span>, String)> = Vec::new();
    for line in messages.lines() {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if json.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(message) = json.get("message") else { continue };
        if let Some(rendered) = message.get("rendered").and_then(|r| r.as_str()) {
            failure.detail.push_str(rendered);
        }
        if !message.get("level").and_then(|l| l.as_str()).is_some_and(|l| l.starts_with("error")) {
            continue;
        }
        let text = message.get("message").and_then(|m| m.as_str()).unwrap_or("");
        if text.starts_with("aborting due to") {
            continue;
        }
        let rustc_code = message.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str());
        let diagnostic = if text.starts_with("linking with") {
            link_failure(message)
        } else if rustc_code == Some("E0463") && (text.contains("`std`") || text.contains("`core`")) {
            missing_std(message)
        } else {
            failure.exit_code = crate::ice::ICE_EXIT_CODE;
            invalid_rust(message, rustc_code, text, map, &mut failure.sources)
        };
        let key = (diagnostic.code, diagnostic.primary_span, diagnostic.message.clone());
        if !seen.contains(&key) {
            seen.push(key);
            failure.diagnostics.push(diagnostic);
        }
    }
    (!failure.diagnostics.is_empty()).then_some(failure)
}

/// What a rustc error code means, said the way a Jux programmer would.
fn jux_words(rustc_code: Option<&str>) -> &'static str {
    match rustc_code {
        Some("E0382" | "E0505") => "value used after it was moved",
        Some("E0499" | "E0502") => "object borrowed twice",
        Some("E0597" | "E0716") => "temporary dropped while in use",
        _ => "the Rust generated for this code does not compile",
    }
}

/// `E0900`: rustc rejected the emitted Rust.
fn invalid_rust(
    message: &serde_json::Value,
    rustc_code: Option<&str>,
    text: &str,
    map: &SourceMap,
    sources: &mut Vec<SourceFile>,
) -> Diagnostic {
    let words = jux_words(rustc_code);
    let mut d = Diagnostic::error(Code::E0900_BackendEmittedInvalidRust, format!("internal compiler error: {words}"));
    let rustc = match rustc_code {
        Some(c) => format!("error[{c}]"),
        None => "error".to_string(),
    };
    let primary = primary_span(message);
    // The cause, on a line of its own: the one-line formats append exactly
    // this note to the E0900 line (LEAKS L27), so it carries rustc's first
    // line and nothing else.
    let first_line = text.lines().next().unwrap_or("");
    d.notes.push(format!("rustc reported {rustc}: {first_line}"));
    if let Some((file, line, col)) = &primary {
        d.notes.push(format!("rustc's error is at {file}:{line}:{col} of the generated crate"));
    }
    if let Some((file, line, _)) = &primary {
        if let Some(entry) = map.lookup(file, *line) {
            match locate(&entry.jux_path, entry.jux_line, entry.jux_col, sources) {
                Some((span, index)) => d = d.with_span(span).with_file(index),
                None => d.notes.push(format!(
                    "the failing code was generated for {}:{}:{}",
                    entry.jux_path, entry.jux_line, entry.jux_col
                )),
            }
        }
    }
    let borrow = matches!(rustc_code, Some("E0382" | "E0505" | "E0499" | "E0502" | "E0597" | "E0716"));
    d.notes.push(if borrow {
        "this is a bug in the Jux compiler, not in your program: Jux has no borrow checker a \
         program can fail (ERRATA E23), so the compiler should never generate this"
            .to_string()
    } else {
        "this is a bug in the Jux compiler, not in your program".to_string()
    });
    d.with_help(format!(
        "please report it at {}, with the source that triggered it; `--verbose` shows the full report",
        crate::ice::ISSUES_URL
    ))
}

/// `E0906`: the linker failed. The linker's own explanation is one line
/// somewhere in the notes; the command line around it is not worth printing.
fn link_failure(message: &serde_json::Value) -> Diagnostic {
    let mut lines: Vec<String> = Vec::new();
    if let Some(children) = message.get("children").and_then(|c| c.as_array()) {
        for child in children {
            if let Some(m) = child.get("message").and_then(|m| m.as_str()) {
                lines.extend(m.lines().map(|l| l.trim().to_string()));
            }
        }
    }
    let reason = lines.iter().find(|l| is_linker_reason(l)).cloned();
    let lib = reason.as_deref().and_then(missing_library);
    let summary = match &lib {
        Some(lib) => format!("could not link library `{lib}`"),
        None => "the program could not be linked".to_string(),
    };
    let mut d = Diagnostic::error(Code::E0906_LinkFailed, summary);
    if let Some(reason) = reason {
        d.notes.push(format!("the linker said: {reason}"));
    }
    let help = match &lib {
        Some(lib) => format!(
            "install the library, or point `[ffi.{lib}]`'s `lib_path` in jux.toml at the directory \
             that holds it; `--verbose` shows the full linker command"
        ),
        None => "`--verbose` shows the full linker command".to_string(),
    };
    d.with_help(help)
}

/// Whether a linker output line is the one that says what went wrong.
fn is_linker_reason(line: &str) -> bool {
    const MARKS: &[&str] = &[
        "fatal error LNK",
        "error LNK",
        "cannot find -l",
        "unable to find library",
        "library not found for",
        "undefined reference to",
        "unresolved external symbol",
        "ld: error",
        "ld returned",
    ];
    MARKS.iter().any(|m| line.contains(m))
}

/// The library a linker line says it could not find: `-lfoo`, `foo.lib`.
fn missing_library(line: &str) -> Option<String> {
    for marker in ["cannot find -l", "library not found for -l", "unable to find library -l"] {
        if let Some(rest) = line.split(marker).nth(1) {
            let name: String = rest.chars().take_while(|c| !c.is_whitespace() && *c != ':' && *c != '\'').collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    // MSVC: `cannot open input file 'foo.lib'`.
    let rest = line.split("cannot open input file '").nth(1)?;
    let file = rest.split('\'').next()?;
    Some(file.strip_suffix(".lib").unwrap_or(file).to_string())
}

/// `E0904`, from rustc: the standard library for the target is not there.
fn missing_std(message: &serde_json::Value) -> Diagnostic {
    let mut d = Diagnostic::error(
        Code::E0904_TargetNotInstalled,
        "the Rust standard library for the build target is not installed",
    );
    if let Some(note) = message
        .get("children")
        .and_then(|c| c.as_array())
        .and_then(|c| c.iter().filter_map(|c| c.get("message").and_then(|m| m.as_str())).find(|m| m.contains("target")))
    {
        d.notes.push(format!("the build tools said: {note}"));
    }
    d.with_help("install it with `rustup target add <target>`")
}

/// The primary span of a rustc message, as `(file, line, column)`. A span
/// inside a macro expansion is reported at the macro's call site, which is
/// where the emitted code is.
fn primary_span(message: &serde_json::Value) -> Option<(String, u32, u32)> {
    let spans = message.get("spans")?.as_array()?;
    let span = spans.iter().find(|s| s.get("is_primary").and_then(|p| p.as_bool()) == Some(true))?;
    let file = span.get("file_name")?.as_str()?.to_string();
    let line = span.get("line_start")?.as_u64()? as u32;
    let col = span.get("column_start")?.as_u64()? as u32;
    Some((file, line, col))
}

/// A span at `line:col` of the `.jux` file at `path`, reading the file into
/// `sources` the first time it is needed. `None` when the file cannot be read,
/// which is the case for the embedded standard library.
pub(crate) fn locate(path: &str, line: u32, col: u32, sources: &mut Vec<SourceFile>) -> Option<(Span, usize)> {
    let index = match sources.iter().position(|s| s.path() == Path::new(path)) {
        Some(i) => i,
        None => {
            let text = std::fs::read_to_string(path).ok()?;
            sources.push(SourceFile::new(path, text));
            sources.len() - 1
        }
    };
    let text = sources[index].contents();
    let line_start: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1) as usize)
        .map(str::len)
        .sum();
    let line_text = text[line_start.min(text.len())..].lines().next().unwrap_or("");
    let start = (line_start + line_text.char_indices().nth(col.saturating_sub(1) as usize).map_or(0, |(i, _)| i))
        .min(text.len());
    // The marker names the start of a statement; underline its first word.
    let word = text[start..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').map(char::len_utf8).sum::<usize>();
    let end = (start + word.max(1)).min(text.len());
    Some((Span::in_file(start as u32, end as u32, index as u32), index))
}

/// Check, before cargo runs, that `--target <triple>` can be built for: rustc
/// knows the triple and its standard library is installed. Without this an
/// uninstalled target was one rustc `E0463` per crate in the build.
pub(crate) fn preflight_target(triple: &str, crate_dir: &Path) -> Result<(), BuildFailure> {
    use std::sync::Mutex;
    // Checked once per target per process: a workspace build asks per package.
    static CHECKED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    if CHECKED.lock().is_ok_and(|c| c.iter().any(|t| t == triple)) {
        return Ok(());
    }
    let output = std::process::Command::new("rustc")
        .args(["--print", "target-libdir", "--target", triple])
        .current_dir(crate_dir)
        .output();
    // No rustc to ask: cargo will say so itself, in a moment.
    let Ok(output) = output else { return Ok(()) };
    let failure = if !output.status.success() {
        Some(
            Diagnostic::error(Code::E0904_TargetNotInstalled, format!("`{triple}` is not a target Jux can build for"))
                .with_help("`rustup target list` shows the targets there are"),
        )
    } else {
        let dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let has_std = std::fs::read_dir(&dir)
            .map(|entries| entries.flatten().any(|e| e.file_name().to_string_lossy().starts_with("libstd-")))
            .unwrap_or(false);
        (!has_std).then(|| {
            Diagnostic::error(
                Code::E0904_TargetNotInstalled,
                format!("the Rust standard library for target `{triple}` is not installed"),
            )
            .with_help(format!("install it with `rustup target add {triple}`"))
        })
    };
    match failure {
        Some(d) => Err(BuildFailure::plain(d)),
        None => {
            if let Ok(mut c) = CHECKED.lock() {
                c.push(triple.to_string());
            }
            Ok(())
        }
    }
}

/// Whether the build targets an Apple platform, the only place a framework
/// exists.
pub(crate) fn target_is_apple(triple: Option<&str>) -> bool {
    match triple {
        Some(t) => t.contains("-apple-"),
        None => cfg!(target_vendor = "apple"),
    }
}

/// Sort the `[ffi.*]` entries of a crate about to be built: an entry no
/// `@extern` in `sources` names is dropped with a `W0906` warning (it used to
/// be linked anyway, and an unused `linkage = "framework"` entry failed every
/// non-Apple build), and a framework entry on a non-Apple target is `E0908`.
pub(crate) fn check_ffi_links(
    links: &mut Vec<juxc_backend_rust::FfiLink>,
    sources: &[(String, String)],
    triple: Option<&str>,
    lints: &crate::lints::LintConfig,
) -> Result<Vec<Diagnostic>, BuildFailure> {
    let mut warnings = Vec::new();
    links.retain(|link| {
        let attr = format!("#[link(name = \"{}\")]", link.lib);
        let used = sources.iter().any(|(path, text)| path.ends_with(".rs") && text.contains(&attr));
        if !used {
            warnings.push(
                Diagnostic::warning(
                    Code::W0906_UnusedFfiEntry,
                    format!("`[ffi.{}]` in jux.toml is not linked: no `@extern(lib = \"{}\")` names it", link.lib, link.lib),
                )
                .with_help("remove the entry, or add the `@extern` block that uses it"),
            );
        }
        used
    });
    // `[lints]` and `-Werror` reach this warning like any other (§D.5.4): it
    // can be allowed, or denied into an error that stops the build here.
    crate::lints::apply_to_manifest(&mut warnings, lints, crate::lints::deny_warnings_requested());
    if warnings.iter().any(|d| d.severity == juxc_diagnostics::Severity::Error) {
        return Err(BuildFailure { diagnostics: warnings, sources: Vec::new(), detail: String::new(), exit_code: 1 });
    }
    if !target_is_apple(triple) {
        if let Some(link) = links.iter().find(|l| l.kind.as_deref() == Some("framework")) {
            let target = triple.map(|t| format!("`{t}`")).unwrap_or_else(|| "this platform".to_string());
            // The warnings found on the way out go with the error.
            let mut failure = BuildFailure::plain(
                Diagnostic::error(
                    Code::E0908_LinkageUnavailable,
                    format!(
                        "`[ffi.{}]` asks for `linkage = \"framework\"`, and {target} has no frameworks: \
                         they exist only on Apple platforms",
                        link.lib
                    ),
                )
                .with_help("use the default dynamic linkage here, or build this program for an Apple target"),
            );
            failure.diagnostics.splice(0..0, warnings);
            return Err(failure);
        }
    }
    Ok(warnings)
}

/// Print build warnings the driver found itself, one line each, on stderr.
pub(crate) fn print_warnings(warnings: &[Diagnostic]) {
    if warnings.is_empty() {
        return;
    }
    let text = crate::render::render_text(warnings, &[], crate::render::DiagnosticFormat::Short, false);
    eprint!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    const BORROW: &str = r#"{"reason":"compiler-message","message":{"rendered":"error[E0382]: use of moved value: `a`\n","children":[],"code":{"code":"E0382","explanation":"..."},"level":"error","message":"use of moved value: `a`","spans":[{"file_name":"src\\main.rs","is_primary":true,"line_start":6,"column_start":16}]}}"#;
    const LINK: &str = r#"{"reason":"compiler-message","message":{"rendered":"error: linking with `link.exe` failed\n","children":[{"level":"note","message":"\"link.exe\" \"/NOLOGO\" \"nosuchlib.lib\""},{"level":"note","message":"LINK : fatal error LNK1181: cannot open input file 'nosuchlib.lib'\r\n"}],"code":null,"level":"error","message":"linking with `link.exe` failed: exit code: 1181","spans":[]}}"#;
    const EMITTED: &str = "fn main() {\n// JUX:x.jux:3:5\n    let a = 1;\n    let b = a;\n// JUX:x.jux:5:5\n    use_it(a);\n}\n";

    #[test]
    fn a_borrow_error_becomes_an_ice_in_jux_words() {
        let f = from_cargo_messages(BORROW, &[("src/main.rs", EMITTED)]).expect("a failure");
        assert_eq!(f.exit_code, crate::ice::ICE_EXIT_CODE);
        let d = &f.diagnostics[0];
        assert_eq!(d.code, Code::E0900_BackendEmittedInvalidRust);
        assert_eq!(d.message, "internal compiler error: value used after it was moved");
        assert!(d.notes[0].starts_with("rustc reported error[E0382]: use of moved value: `a`"), "{:?}", d.notes);
        // `x.jux` cannot be read here, so the location is a note.
        assert!(d.notes.iter().any(|n| n.contains("x.jux:5:5")), "{:?}", d.notes);
        assert!(d.help[0].contains("/issues"), "{:?}", d.help);
    }

    #[test]
    fn a_link_error_names_the_library_and_the_one_line() {
        let f = from_cargo_messages(LINK, &[]).expect("a failure");
        assert_eq!(f.exit_code, 1);
        let d = &f.diagnostics[0];
        assert_eq!(d.code, Code::E0906_LinkFailed);
        assert_eq!(d.message, "could not link library `nosuchlib`");
        assert_eq!(d.notes, vec!["the linker said: LINK : fatal error LNK1181: cannot open input file 'nosuchlib.lib'"]);
        assert!(!format!("{d:?}").contains("/NOLOGO"), "the command line stays behind --verbose");
        assert!(f.detail.contains("linking with"));
    }

    #[test]
    fn gnu_and_apple_linkers_name_the_library_too() {
        assert_eq!(missing_library("/usr/bin/ld: cannot find -lsqlite4: No such file").as_deref(), Some("sqlite4"));
        assert_eq!(missing_library("ld: library not found for -lfoo").as_deref(), Some("foo"));
    }

    #[test]
    fn a_build_with_no_compiler_error_is_not_dressed_up() {
        assert!(from_cargo_messages("{\"reason\":\"build-finished\",\"success\":false}\n", &[]).is_none());
    }

    #[test]
    fn a_framework_on_a_non_apple_target_is_e0908_and_an_unused_entry_is_dropped() {
        let link = |lib: &str, kind: Option<&str>| juxc_backend_rust::FfiLink {
            lib: lib.to_string(),
            kind: kind.map(str::to_string),
            search_paths: Vec::new(),
            extra_libs: Vec::new(),
        };
        let sources = vec![("src/main.rs".to_string(), "#[link(name = \"Cocoa\")]\nextern \"C\" {}\n".to_string())];

        let mut unused = vec![link("Metal", Some("framework"))];
        let lints = crate::lints::LintConfig::default();
        let warnings = check_ffi_links(&mut unused, &sources, Some("x86_64-unknown-linux-gnu"), &lints).expect("only a warning");
        assert!(unused.is_empty());
        assert_eq!(warnings[0].code, Code::W0906_UnusedFfiEntry);

        let mut used = vec![link("Cocoa", Some("framework"))];
        let err = check_ffi_links(&mut used, &sources, Some("x86_64-unknown-linux-gnu"), &lints).unwrap_err();
        assert_eq!(err.diagnostics[0].code, Code::E0908_LinkageUnavailable);
        assert!(check_ffi_links(&mut used, &sources, Some("aarch64-apple-darwin"), &lints).is_ok());
    }
}
