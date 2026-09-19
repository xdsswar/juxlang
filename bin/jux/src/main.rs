//! `jux` — the Jux project tool.
//!
//! Per `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.11 and §B.15: `jux` is the
//! Cargo-equivalent project tool. It reads `jux.toml`, resolves
//! dependencies, and dispatches `juxc` invocations. Day-to-day use is
//! `jux`; `juxc` is the file-level compiler invoked by `jux`, by the
//! language server, and by foreign build systems.
//!
//! ## Project mode vs single-file mode
//!
//! **Project mode** — `jux run`/`build`/`check` with no file argument —
//! reads `./jux.toml` and builds the manifest's `[lib]`/`[[bin]]`
//! targets (or every `[workspace]` member in dependency order, §B.7),
//! resolving `path` and `git` dependencies (§B.2.2).
//!
//! **Single-file mode** — `jux run <file.jux>` etc. — dispatches
//! through the same `juxc-driver` library that `juxc` uses, so the IDE
//! workflow advertised by the spec (`jux run examples/hello.jux`)
//! works without a manifest.
//!
//! ## Commands
//!
//! - `jux run [file]` — compile, build, execute. Forwards stdio + exit code.
//! - `jux build [file]` — compile + cargo build, don't execute.
//! - `jux check [file]` — lex/parse/resolve/typecheck only, no codegen.
//! - `jux update` — re-fetch the project's git dependencies (§B.2.2).
//! - `jux new [--lib|--workspace] <name>`, `jux init` — scaffold a project (§B.15.1).
//! - `jux test` — run `@Test` functions.
//! - `jux clean`, `jux add`, `jux remove`, `jux tree` — see `project_cmds`.

mod project_cmds;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use juxc_diagnostics::{Diagnostic, Severity};

#[derive(Parser, Debug)]
#[command(name = "jux", version, about = "The Jux project tool")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
    /// Path to a `jux.toml` (or the directory containing one) to act on,
    /// instead of searching upward from the current directory. Lets the
    /// IDE tool window operate on a module without changing the process
    /// working directory. Ignored in single-file mode (the file is explicit).
    #[arg(long, global = true, value_name = "PATH")]
    manifest_path: Option<PathBuf>,
    /// Diagnostic format (JUX-DIAGNOSTICS-ADDENDUM §D.1, §D.2): `human`,
    /// `compact`, `short`, `line` or `json`. Default: `human` on a terminal,
    /// else `line`.
    #[arg(long, global = true, value_name = "FORMAT")]
    diagnostic_format: Option<String>,
    /// Color the `human` format: `auto`, `always` or `never`.
    #[arg(long, global = true, value_name = "WHEN", default_value = "auto")]
    color: String,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    /// Create a new Jux project (§B.15.1): a binary by default, a library
    /// with `--lib`, an empty workspace with `--workspace`.
    New {
        /// Name of the project directory to create.
        name: String,
        /// Scaffold a library (`[lib]`, code under `src/<name>/`, a test).
        #[arg(long, conflicts_with = "workspace")]
        lib: bool,
        /// Scaffold a workspace root with an empty member list.
        #[arg(long)]
        workspace: bool,
    },
    /// Add a `jux.toml` to the current directory (§B.15).
    Init,
    /// Delete the build output: `target/`, and each workspace member's
    /// `target/` (§B.15).
    Clean,
    /// Add or replace a dependency in `jux.toml` (§B.10.5). The name may carry
    /// a version: `jux add com.x.json@1.0`, `jux add rust.rand@0.9`.
    Add {
        /// `<name>` or `<name>@<version>`.
        spec: String,
        /// A SemVer requirement (same as `@<version>`).
        #[arg(long)]
        version: Option<String>,
        /// Depend on a local directory.
        #[arg(long, conflicts_with = "git")]
        path: Option<String>,
        /// Depend on a remote repository.
        #[arg(long)]
        git: Option<String>,
        /// Branch to track.
        #[arg(long)]
        branch: Option<String>,
        /// Tag to pin.
        #[arg(long)]
        tag: Option<String>,
        /// Revision to pin.
        #[arg(long)]
        rev: Option<String>,
        /// Features to enable on the dependency, comma-separated.
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
    },
    /// Remove a dependency from `jux.toml` (§B.10.5).
    Remove {
        /// The dependency's name.
        name: String,
    },
    /// Print the dependency tree (§B.10.5).
    Tree,
    /// Explain a diagnostic code, offline (JUX-DIAGNOSTICS-ADDENDUM §D.5.3).
    Explain {
        /// The code, e.g. `E0413`.
        code: String,
    },
    /// Generate HTML documentation from doc comments into `target/doc/`
    /// (JUX-LANG-V1 §3.5, §12.5), and run the ```` ```jux ```` examples in
    /// them as tests.
    Doc {
        /// Open the generated index in the browser afterwards.
        #[arg(long)]
        open: bool,
        /// In a workspace, document only this member.
        #[arg(short = 'p', long)]
        package: Option<String>,
        /// Write the pages without compiling and running the examples.
        #[arg(long)]
        no_doctests: bool,
    },
    /// Type-check the project (or a single file) without producing a binary.
    /// (§B.15 — `jux check`.)
    Check {
        /// Optional `.jux` file. When omitted, acts on the project whose
        /// `jux.toml` is found upward from here (or given by --manifest-path).
        file: Option<PathBuf>,
        /// In a workspace, type-check only this member package (by package
        /// name or its last segment). Ignored in single-file mode.
        #[arg(short = 'p', long)]
        package: Option<String>,
        /// Type-check against the given Rust target triple (forwards
        /// `--target` to the inner cargo metadata / artifact-path logic).
        /// The toolchain must be installed: `rustup target add <triple>`.
        #[arg(long)]
        target: Option<String>,
        /// Enable these features of the package, comma-separated, on top of
        /// its `default` set (§B.8; read by `@cfg(feature = "...")`).
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
        /// Leave the package's `default` features off.
        #[arg(long)]
        no_default_features: bool,
        /// Type-check under this `[profile.<name>]` (§B.9); it decides
        /// `cfg(debug)` / `cfg(release)`.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
    /// Build the project (or a single file). (§B.15 — `jux build`.)
    Build {
        /// Optional `.jux` file.
        file: Option<PathBuf>,
        /// Where to emit the generated Rust crate. Defaults to
        /// `<input-parent>/target/.rust-build/`, or `<project>/target/.rust-build/`
        /// for a project (a whole workspace always uses the latter).
        #[arg(long)]
        emit_dir: Option<PathBuf>,
        /// Build the emitted program with optimizations
        /// (forwards `--release` to the inner `cargo build`).
        #[arg(long)]
        release: bool,
        /// In a workspace, build only this member package (by package name
        /// or its last segment).
        #[arg(short = 'p', long)]
        package: Option<String>,
        /// Build only this `[[bin]]` target (by name). Mutually exclusive
        /// with `--lib`.
        #[arg(long, conflicts_with = "lib")]
        bin: Option<String>,
        /// Build only the `[lib]` target. Mutually exclusive with `--bin`.
        #[arg(long)]
        lib: bool,
        /// Build the program `examples/<NAME>.jux` (or `examples/<NAME>/`)
        /// against this package's library code (§B.1.3).
        #[arg(long, value_name = "NAME", conflicts_with_all = ["bin", "lib", "examples"])]
        example: Option<String>,
        /// Build every program under `examples/` (§B.1.3).
        #[arg(long, conflicts_with_all = ["bin", "lib"])]
        examples: bool,
        /// Cross-compile for the given Rust target triple (forwards
        /// `--target` to the inner `cargo build`). The toolchain must
        /// be installed: `rustup target add <triple>`.
        #[arg(long)]
        target: Option<String>,
        /// Enable these features of the package, comma-separated, on top of
        /// its `default` set (§B.8; read by `@cfg(feature = "...")`).
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
        /// Leave the package's `default` features off.
        #[arg(long)]
        no_default_features: bool,
        /// Build with this `[profile.<name>]` (§B.9): a custom profile from
        /// `jux.toml`, or one of `dev`, `release`, `test`, `bench`. Replaces
        /// `--release`, which is the same as `--profile release`.
        #[arg(long, value_name = "NAME", conflicts_with = "release")]
        profile: Option<String>,
    },
    /// Build and run the project (or a single file). (§B.15 — `jux run`.)
    Run {
        /// Optional `.jux` file. When omitted, acts on the project whose
        /// `jux.toml` is found upward from here (or given by --manifest-path).
        file: Option<PathBuf>,
        /// Where to emit the generated Rust crate. Defaults to
        /// `<input-parent>/target/.rust-build/`, or `<project>/target/.rust-build/`
        /// for a project (a whole workspace always uses the latter).
        #[arg(long)]
        emit_dir: Option<PathBuf>,
        /// Build the emitted program with optimizations
        /// (forwards `--release` to the inner `cargo build`).
        #[arg(long)]
        release: bool,
        /// In a workspace, run a binary from this member package (by package
        /// name or its last segment).
        #[arg(short = 'p', long)]
        package: Option<String>,
        /// Run this specific `[[bin]]` target (by name) instead of the
        /// package's first binary.
        #[arg(long)]
        bin: Option<String>,
        /// Run the program `examples/<NAME>.jux` (or `examples/<NAME>/`)
        /// instead of a `[[bin]]` (§B.15.3).
        #[arg(long, value_name = "NAME", conflicts_with = "bin")]
        example: Option<String>,
        /// Arguments for the program itself, after a `--` separator.
        ///
        /// `main(String[] args)` has always been a legal entry point; until
        /// now there was no way to give it anything.
        #[arg(last = true)]
        args: Vec<String>,
        /// Cross-compile + run for the given Rust target triple. The
        /// toolchain must be installed: `rustup target add <triple>`.
        #[arg(long)]
        target: Option<String>,
        /// Enable these features of the package, comma-separated, on top of
        /// its `default` set (§B.8; read by `@cfg(feature = "...")`).
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
        /// Leave the package's `default` features off.
        #[arg(long)]
        no_default_features: bool,
        /// Build with this `[profile.<name>]` (§B.9): a custom profile from
        /// `jux.toml`, or one of `dev`, `release`, `test`, `bench`. Replaces
        /// `--release`, which is the same as `--profile release`.
        #[arg(long, value_name = "NAME", conflicts_with = "release")]
        profile: Option<String>,
    },
    /// Run tests (JUX-TESTING-ADDENDUM §TS.2/§TS.8).
    Test {
        /// Optional substring filter: only tests whose
        /// package-qualified name contains the pattern run.
        pattern: Option<String>,
        /// In a workspace, test only this member package (by package name
        /// or its last segment).
        #[arg(short = 'p', long)]
        package: Option<String>,
        /// Build the test runner with optimizations. The `assert`
        /// builtin stays checked under `jux test` either way.
        #[arg(long)]
        release: bool,
        /// Run only the ```` ```jux ```` examples in doc comments.
        #[arg(long)]
        doc: bool,
        /// Enable these features of the package, comma-separated, on top of
        /// its `default` set (§B.8; read by `@cfg(feature = "...")`).
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
        /// Leave the package's `default` features off.
        #[arg(long)]
        no_default_features: bool,
        /// Build with this `[profile.<name>]` (§B.9): a custom profile from
        /// `jux.toml`, or one of `dev`, `release`, `test`, `bench`. Replaces
        /// `--release`, which is the same as `--profile release`.
        #[arg(long, value_name = "NAME", conflicts_with = "release")]
        profile: Option<String>,
    },
    /// Re-fetch the project's git dependencies (§B.2.2). Branch-pinned
    /// deps pick up new commits; tag/rev pins re-validate. Without
    /// this, a cached checkout is reused as-is on every build (the
    /// cache is the pin until the §B.6 lockfile lands).
    Update,
    /// Emit machine-readable project metadata as JSON: workspace members,
    /// each package's targets (`[lib]`/`[[bin]]` with kinds + resolved
    /// artifact paths), declared dependencies (with source), and profiles.
    /// Consumed by the IntelliJ "Jux Project" tool window so the tree is
    /// authoritative instead of hand-parsing `jux.toml`.
    Metadata {
        /// Output format. Only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
        /// Report artifact paths for this cross-compile target triple
        /// (adds the `target/<triple>/…` segment). Defaults to the host.
        #[arg(long)]
        target: Option<String>,
    },
    /// Inspect cross-compilation targets (analogous to `rustup target`).
    Target {
        #[command(subcommand)]
        cmd: TargetCmd,
    },
}

/// Sub-commands of `jux target`.
#[derive(Subcommand, Debug)]
enum TargetCmd {
    /// List the cross-compile target triples (installed ones are marked).
    /// Thin wrapper over `rustup target list`; pass `--installed` to limit
    /// the output to installed toolchains. Drives the IDE's triple picker.
    List {
        /// Only list installed target triples.
        #[arg(long)]
        installed: bool,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    // What `jux` was pointed at, kept for an ICE report.
    let inputs = ice_inputs(&cli);
    // Same contract as `juxc`: a panic in here is a compiler bug and reports
    // itself as one. See `juxc_driver::ice`.
    juxc_driver::ice::guard("jux", &inputs, move || {
        // Same reason as `juxc`: the front end runs in-process here, it recurses
        // with source nesting, and its frames are large enough that a default
        // main stack aborts the process around 60 levels deep. See
        // `juxc_driver::big_stack`.
        juxc_driver::big_stack::run(move || {
            juxc_driver::ice::selftest_trip();
            run_cli(cli)
        })
    })
}

/// What this invocation was pointed at, for an internal-compiler-error report.
///
/// Single-file mode carries the `.jux` path in the sub-command, project mode
/// carries a manifest (explicit or, most often, none at all because it was
/// found by walking up from the working directory). Either is worth naming: an
/// ICE report that says only "jux crashed" cannot be reproduced.
fn ice_inputs(cli: &Cli) -> Vec<PathBuf> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let file = match &cli.command {
        CliCommand::Check { file, .. }
        | CliCommand::Build { file, .. }
        | CliCommand::Run { file, .. } => file.clone(),
        // The rest are project-wide or take no path at all.
        CliCommand::New { .. }
        | CliCommand::Init
        | CliCommand::Clean
        | CliCommand::Add { .. }
        | CliCommand::Remove { .. }
        | CliCommand::Tree
        | CliCommand::Explain { .. }
        | CliCommand::Doc { .. }
        | CliCommand::Test { .. }
        | CliCommand::Update
        | CliCommand::Metadata { .. }
        | CliCommand::Target { .. } => None,
    };
    inputs.extend(file);
    inputs.extend(cli.manifest_path.clone());
    inputs
}

/// Dispatch one parsed command line. Split out of `main` so the whole of it
/// runs on the large-stack thread.
fn run_cli(cli: Cli) -> Result<ExitCode> {
    if let Err(msg) = set_diagnostic_style(cli.diagnostic_format.as_deref(), &cli.color) {
        eprintln!("jux: {msg}");
        return Ok(ExitCode::from(2));
    }
    // Resolve the project root once: an explicit `--manifest-path`, else the
    // nearest `jux.toml` walking up from the cwd. `None` when no manifest is
    // found (project-mode commands report their own "no jux.toml" error).
    let root = resolve_project_root(cli.manifest_path.as_deref());
    match cli.command {
        CliCommand::New { name, lib, workspace } => {
            let kind = if workspace {
                project_cmds::NewKind::Workspace
            } else if lib {
                project_cmds::NewKind::Lib
            } else {
                project_cmds::NewKind::Bin
            };
            project_cmds::cmd_new(&name, kind)
        }
        CliCommand::Init => project_cmds::cmd_init(Path::new(".")),
        CliCommand::Clean => with_root(root, "clean", project_cmds::cmd_clean),
        CliCommand::Add { spec, version, path, git, branch, tag, rev, features } => {
            let source = project_cmds::DepSource { version, path, git, branch, tag, rev, features };
            with_root(root, "add", |r| project_cmds::cmd_add(r, &spec, source))
        }
        CliCommand::Remove { name } => with_root(root, "remove", |r| project_cmds::cmd_remove(r, &name)),
        CliCommand::Tree => with_root(root, "tree", project_cmds::cmd_tree),
        CliCommand::Explain { code } => Ok(match juxc_driver::explain::explain(&code) {
            Some(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!(
                    "jux: `{}` is not a diagnostic code this compiler knows",
                    juxc_driver::explain::normalize(&code),
                );
                ExitCode::from(1)
            }
        }),
        CliCommand::Doc { open, package, no_doctests } => {
            with_root(root, "doc", |r| cmd_doc(r, package.as_deref(), open, !no_doctests))
        }
        CliCommand::Test { pattern, package, release, doc, features, no_default_features, profile } => {
            set_features(features, no_default_features);
            set_profile(profile);
            if doc {
                return with_root(root, "test --doc", |r| cmd_doctest(r, package.as_deref(), release));
            }
            cmd_test(root, package.as_deref(), pattern, release)
        }
        CliCommand::Update       => cmd_update(root),
        CliCommand::Metadata { format, target } => {
            set_cross_target(target);
            cmd_metadata(root, &format)
        }
        CliCommand::Target { cmd } => match cmd {
            TargetCmd::List { installed } => cmd_target_list(installed),
        },
        CliCommand::Check { file, package, target, features, no_default_features, profile } => {
            set_cross_target(target);
            set_features(features, no_default_features);
            set_profile(profile);
            let sel = Selection { package, ..Selection::default() };
            run_single_or_project(root, file, Action::Check, None, false, sel)
        }
        CliCommand::Build { file, emit_dir, release, package, bin, lib, example, examples, target, features, no_default_features, profile } => {
            set_cross_target(target);
            set_features(features, no_default_features);
            set_profile(profile);
            let sel = Selection { package, bin, lib, example, examples };
            run_single_or_project(root, file, Action::Build, emit_dir, release, sel)
        }
        CliCommand::Run { file, emit_dir, release, package, bin, example, args, target, features, no_default_features, profile } => {
            set_cross_target(target);
            set_features(features, no_default_features);
            set_profile(profile);
            set_program_args(args);
            let sel = Selection { package, bin, lib: false, example, examples: false };
            run_single_or_project(root, file, Action::Run, emit_dir, release, sel)
        }
    }
}

/// Which package / target the user selected in project mode (`-p`, `--bin`,
/// `--lib`). Empty/`None` fields mean "no selection" (build the whole workspace
/// or all of a package's targets), preserving the no-flag behavior.
#[derive(Debug, Clone, Default)]
struct Selection {
    /// `-p, --package <name>` — restrict to one workspace member.
    package: Option<String>,
    /// `--bin <name>` — restrict to one `[[bin]]` target.
    bin: Option<String>,
    /// `--lib` — restrict to the `[lib]` target.
    lib: bool,
    /// `--example <name>` — build (or run) one program from `examples/`.
    example: Option<String>,
    /// `--examples` — build every program under `examples/`.
    examples: bool,
}

impl Selection {
    /// True when no package/target restriction was requested.
    fn is_empty(&self) -> bool {
        self.package.is_none()
            && self.bin.is_none()
            && !self.lib
            && self.example.is_none()
            && !self.examples
    }
}

/// Apply the CLI `--target <triple>` flag by setting `JUX_TARGET`, which the
/// driver's cargo invocations and artifact-path computations read (see
/// `juxc_driver::cross_target`). A `None` leaves the env untouched so the
/// manifest's `[build] target` default (applied later) can still take effect.
/// Arguments to hand the program being run, from `jux run … -- <args>`.
///
/// Carried in a process-wide slot rather than threaded through every run
/// signature, the same way the cross-compile target already is.
static PROGRAM_ARGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

fn set_program_args(args: Vec<String>) {
    let _ = PROGRAM_ARGS.set(args);
}

fn program_args() -> &'static [String] {
    PROGRAM_ARGS.get().map(|v| v.as_slice()).unwrap_or(&[])
}

/// Apply `--features` / `--no-default-features` the way `--target` is applied:
/// through the environment the driver reads when it computes a package's
/// enabled features (`juxc_driver::project::cfg_facts_for`).
fn set_features(features: Vec<String>, no_default_features: bool) {
    if !features.is_empty() {
        std::env::set_var("JUX_FEATURES", features.join(","));
    }
    if no_default_features {
        std::env::set_var("JUX_NO_DEFAULT_FEATURES", "1");
    }
}

fn set_cross_target(triple: Option<String>) {
    if let Some(t) = triple {
        std::env::set_var("JUX_TARGET", t);
    }
}

/// Record `--profile <name>` in `JUX_PROFILE`, which the driver's cargo
/// invocations read (`juxc_driver::cargo_profile_args`), the same way
/// `--target` travels through `JUX_TARGET`.
fn set_profile(profile: Option<String>) {
    if let Some(p) = profile {
        std::env::set_var("JUX_PROFILE", p);
    }
}

/// The `--profile` the user asked for, if any.
fn selected_profile() -> Option<String> {
    std::env::var("JUX_PROFILE").ok().filter(|p| !p.trim().is_empty())
}

/// Decide the build type for a project build: the named `--profile` when one
/// was given (validated against the manifest, which owns custom profiles),
/// else the manifest's default (`[build] optimization`) overridden by
/// `--release`. `Err` carries the message for an unknown profile.
fn resolve_release(manifest: &juxc_driver::Manifest, cli_release: bool) -> std::result::Result<bool, String> {
    let Some(name) = selected_profile() else {
        return Ok(manifest.effective_release(cli_release));
    };
    manifest.profile_is_optimized(&name).ok_or_else(|| {
        format!(
            "no profile `{name}` in {}; available: {}",
            manifest.project_root.join("jux.toml").display(),
            manifest.selectable_profiles().join(", "),
        )
    })
}

/// The same for a file with no manifest: only the four built-in profiles
/// exist there.
fn resolve_release_without_manifest(cli_release: bool) -> std::result::Result<bool, String> {
    match selected_profile().as_deref() {
        None => Ok(cli_release),
        Some("release") | Some("bench") => Ok(true),
        Some("dev") | Some("test") => Ok(false),
        Some(other) => Err(format!(
            "profile `{other}` needs a project: custom profiles are declared as `[profile.{other}]` in jux.toml"
        )),
    }
}

/// Resolve the project root to act on. With an explicit `--manifest-path`,
/// accept either the `jux.toml` file (use its parent) or a directory (use it
/// directly). Otherwise walk up from the current directory and return the first
/// ancestor that contains a `jux.toml` (Cargo's nearest-manifest behavior), or
/// `None` when none is found.
fn resolve_project_root(manifest_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(mp) = manifest_path {
        if mp.is_dir() {
            return Some(mp.to_path_buf());
        }
        // A file path (existing or not): the project root is its parent.
        // `file_name().is_some()` rejects paths like `/` or `..` with no parent.
        if mp.file_name().is_some() {
            return mp.parent().map(|p| {
                if p.as_os_str().is_empty() {
                    // `--manifest-path jux.toml` → parent is "" → cwd.
                    PathBuf::from(".")
                } else {
                    p.to_path_buf()
                }
            });
        }
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join("jux.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// Run a project command that needs a `jux.toml`, or say how to get one.
fn with_root(
    root: Option<PathBuf>,
    command: &str,
    f: impl FnOnce(&Path) -> Result<ExitCode>,
) -> Result<ExitCode> {
    match root {
        Some(r) => f(&r),
        None => {
            eprintln!(
                "jux: no jux.toml found -- run `jux {command}` from a project root (or pass --manifest-path)",
            );
            Ok(ExitCode::from(1))
        }
    }
}

/// The packages a project-level command acts on: the one named by `-p`, else
/// every workspace member (default members first), else the package itself.
fn packages_for(root: &Path, package: Option<&str>) -> std::result::Result<Vec<juxc_driver::Manifest>, String> {
    let Some(root_manifest) = juxc_driver::Manifest::load(root) else {
        return Err(format!("failed to load {}", root.join("jux.toml").display()));
    };
    if root_manifest.workspace_members.is_empty() {
        if let Some(pkg) = package {
            if !package_name_matches(&root_manifest, pkg) {
                return Err(format!("package `{pkg}` not found (this project is `{}`)", root_manifest.package.name));
            }
        }
        return Ok(vec![root_manifest]);
    }
    if let Some(pkg) = package {
        let (_dir, m) = select_member(&root_manifest, root, pkg)?;
        return Ok(vec![juxc_driver::project::with_root_profiles(&m, &root_manifest)]);
    }
    let mut out = Vec::new();
    for member in &root_manifest.workspace_default_members {
        if let Some(m) = juxc_driver::Manifest::load(&root.join(member)) {
            out.push(juxc_driver::project::with_root_profiles(&m, &root_manifest));
        }
    }
    Ok(out)
}

/// `jux doc`: write `target/doc/` from the package's doc comments, then run
/// the examples in them unless `--no-doctests`. A workspace gets one site per
/// member under `target/doc/<member>/` and an index linking them.
fn cmd_doc(root: &Path, package: Option<&str>, open: bool, run_examples: bool) -> Result<ExitCode> {
    let manifests = match packages_for(root, package) {
        Ok(m) => m,
        Err(msg) => {
            eprintln!("jux: {msg}");
            return Ok(ExitCode::from(1));
        }
    };
    let doc_root = root.join("target").join("doc");
    let nested = manifests.len() > 1;
    let mut index_links: Vec<(String, String)> = Vec::new();
    let mut failed = 0usize;
    for manifest in &manifests {
        let sources = juxc_driver::project::collect_dependency_sources(manifest)?;
        let packages = juxc_driver::docgen::collect(&sources);
        let out_dir = if nested {
            doc_root.join(juxc_driver::manifest::default_target_name(&manifest.package.name))
        } else {
            doc_root.clone()
        };
        let written = juxc_driver::docgen::write_site(manifest, &packages, &out_dir)?;
        eprintln!(
            "jux: documented {} item(s) of `{}` at {}",
            written.items,
            manifest.package.name,
            written.index.display(),
        );
        index_links.push((
            manifest.package.name.clone(),
            format!("{}/index.html", juxc_driver::manifest::default_target_name(&manifest.package.name)),
        ));
        if run_examples {
            failed += run_doctests_for(manifest, &packages, root, false)?;
        }
    }
    if nested {
        write_workspace_doc_index(&doc_root, &index_links)?;
    }
    if open {
        open_in_browser(&doc_root.join("index.html"));
    }
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

/// `jux test --doc`: only the doc examples.
fn cmd_doctest(root: &Path, package: Option<&str>, release: bool) -> Result<ExitCode> {
    let manifests = match packages_for(root, package) {
        Ok(m) => m,
        Err(msg) => {
            eprintln!("jux: {msg}");
            return Ok(ExitCode::from(1));
        }
    };
    let mut failed = 0usize;
    for manifest in &manifests {
        let sources = juxc_driver::project::collect_dependency_sources(manifest)?;
        let packages = juxc_driver::docgen::collect(&sources);
        failed += run_doctests_for(manifest, &packages, root, release)?;
    }
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

/// Run one package's doc examples and print a report in the style of the
/// `@Test` runner. Returns how many failed.
fn run_doctests_for(
    manifest: &juxc_driver::Manifest,
    packages: &[juxc_driver::docgen::DocPackage],
    root: &Path,
    release: bool,
) -> Result<usize> {
    let tests = juxc_driver::docgen::doctests(packages);
    if tests.is_empty() {
        return Ok(0);
    }
    let emit_root = root.join("target").join(".rust-build-doctest");
    let (dep_sources, _path_deps) = juxc_driver::project::resolve_package_deps(manifest, &emit_root)?;
    let facts = juxc_driver::project::cfg_facts_for(manifest, release);
    println!("running {} doc example(s) of `{}`", tests.len(), manifest.package.name);
    let results = juxc_driver::docgen::run_doctests(manifest, &dep_sources, &tests, &emit_root, release, &facts)?;
    let mut failed = 0usize;
    for r in &results {
        match &r.failure {
            None => println!("  PASS {} ({})", r.owner, r.location),
            Some(why) => {
                failed += 1;
                println!("  FAIL {} ({})", r.owner, r.location);
                for line in why.lines() {
                    println!("       {line}");
                }
            }
        }
    }
    println!(
        "\ndoc examples: {}. {} passed; {} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        results.len() - failed,
        failed,
    );
    Ok(failed)
}

/// The top-level `target/doc/index.html` of a workspace: a link per member.
fn write_workspace_doc_index(doc_root: &Path, members: &[(String, String)]) -> Result<()> {
    let mut items = String::new();
    for (name, href) in members {
        items.push_str(&format!("<li><a href=\"{href}\">{name}</a></li>\n"));
    }
    let html = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Workspace documentation</title>\n\
         <style>body{{font:15px/1.55 system-ui,sans-serif;max-width:760px;margin:40px auto;padding:0 16px}}</style>\n\
         </head>\n<body>\n<h1>Workspace documentation</h1>\n<ul>\n{items}</ul>\n</body>\n</html>\n",
    );
    std::fs::create_dir_all(doc_root)?;
    std::fs::write(doc_root.join("index.html"), html).context("writing the workspace doc index")
}

/// Best effort: hand `page` to the platform's opener. A failure only prints.
fn open_in_browser(page: &Path) {
    let result = if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", ""]).arg(page).status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(page).status()
    } else {
        Command::new("xdg-open").arg(page).status()
    };
    if result.is_err() {
        eprintln!("jux: could not open a browser; the docs are at {}", page.display());
    }
}

/// `jux update` — re-fetch every git dependency of the current project
/// (and, for a workspace root, of every member). Each fetched checkout
/// replaces its cache entry; a failed fetch falls back to the cached
/// copy with a warning (see `juxc_driver::git_deps`).
fn cmd_update(root: Option<PathBuf>) -> Result<ExitCode> {
    let Some(root) = root else {
        eprintln!("jux: no jux.toml found -- run `jux update` from a project root (or pass --manifest-path)");
        return Ok(ExitCode::from(1));
    };
    let Some(manifest) = juxc_driver::Manifest::load(&root) else {
        eprintln!("jux: failed to load {}", root.join("jux.toml").display());
        return Ok(ExitCode::from(1));
    };
    // The root manifest plus every workspace member's.
    let mut manifests = vec![manifest.clone()];
    for member in &manifest.workspace_members {
        if let Some(m) = juxc_driver::Manifest::load(&root.join(member)) {
            manifests.push(m);
        }
    }
    let mut updated = 0usize;
    let mut failed = 0usize;
    for m in &manifests {
        for dep in &m.dependencies {
            if dep.git.is_none() || dep.path.is_some() {
                // Not a git dep, or path-overridden (§B.5.5).
                continue;
            }
            match juxc_driver::git_deps::fetch_git_dep(dep, true) {
                Ok(dir) => {
                    eprintln!("jux: updated `{}` → {}", dep.name, dir.display());
                    updated += 1;
                }
                Err(e) => {
                    eprintln!("jux: error updating `{}`: {e:#}", dep.name);
                    failed += 1;
                }
            }
        }
    }
    if updated == 0 && failed == 0 {
        eprintln!("jux: no git dependencies to update");
    }
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// `jux target list [--installed]` — list cross-compile target triples via
/// `rustup`. A thin pass-through (rustup already marks installed triples), so
/// the IDE can offer a real picker instead of a free-text triple field. Exits 2
/// with guidance when rustup is unavailable.
fn cmd_target_list(installed: bool) -> Result<ExitCode> {
    let mut cmd = Command::new("rustup");
    cmd.arg("target").arg("list");
    if installed {
        cmd.arg("--installed");
    }
    match cmd.status() {
        Ok(status) => Ok(ExitCode::from(status.code().unwrap_or(1) as u8)),
        Err(e) => {
            eprintln!("jux: could not run `rustup target list`: {e}");
            eprintln!(
                "     install rustup (https://rustup.rs) to manage cross-compile targets",
            );
            Ok(ExitCode::from(2))
        }
    }
}

/// `jux metadata --format json` — emit machine-readable project metadata for the
/// IntelliJ "Jux Project" tool window (see
/// `ide/intellij-plugin/cli-support-request.md` #5): workspace members, each
/// package's targets (with resolved artifact paths), declared dependencies
/// (tagged by source), and profile names. Lets the panel be authoritative
/// instead of hand-parsing `jux.toml`.
fn cmd_metadata(root: Option<PathBuf>, format: &str) -> Result<ExitCode> {
    if format != "json" {
        eprintln!("jux: unsupported metadata format `{format}` (only `json` is supported)");
        return Ok(ExitCode::from(2));
    }
    let Some(root) = root else {
        eprintln!(
            "jux: no jux.toml found -- run from a project root (or pass --manifest-path)",
        );
        return Ok(ExitCode::from(1));
    };
    let Some(root_manifest) = juxc_driver::Manifest::load(&root) else {
        eprintln!("jux: failed to load {}", root.join("jux.toml").display());
        return Ok(ExitCode::from(1));
    };

    // Enumerate packages: every `[workspace] members` entry, plus the root
    // itself when it is a package (declares its own targets) or stands alone.
    let is_workspace = !root_manifest.workspace_members.is_empty();
    let mut packages: Vec<(PathBuf, juxc_driver::Manifest, bool)> = Vec::new();
    if !is_workspace {
        packages.push((root.clone(), root_manifest.clone(), false));
    } else {
        if root_manifest.lib.is_some() || !root_manifest.bins.is_empty() {
            packages.push((root.clone(), root_manifest.clone(), true));
        }
        for member in &root_manifest.workspace_members {
            let mdir = root.join(member);
            if let Some(m) = juxc_driver::Manifest::load(&mdir) {
                packages.push((mdir, m, true));
            }
        }
    }

    // Emitted crates (and thus produced artifacts) for every package — standalone
    // or workspace member — live under the resolved root's `target/.rust-build`,
    // matching `build_package` / `build_workspace`.
    let emit_base = root.join("target").join(".rust-build");
    let pkg_json: Vec<serde_json::Value> = packages
        .iter()
        .map(|(proot, m, is_member)| package_metadata_json(proot, m, *is_member, &emit_base))
        .collect();

    let out = serde_json::json!({
        "workspace_root": path_str(&root),
        "is_workspace": is_workspace,
        // The active cross-compile triple (`--target`/`[build] target`), if any.
        "target": std::env::var("JUX_TARGET").ok().filter(|t| !t.is_empty()),
        "packages": pkg_json,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(ExitCode::SUCCESS)
}

/// Build the JSON object for one package: identity, targets (with EXPECTED
/// artifact paths), dependencies (tagged by source), and profile names.
fn package_metadata_json(
    proot: &Path,
    m: &juxc_driver::Manifest,
    is_member: bool,
    emit_base: &Path,
) -> serde_json::Value {
    use juxc_driver::manifest::GitRef;

    // ---- targets (lib + each bin) ----
    let mut targets: Vec<serde_json::Value> = Vec::new();
    if let Some(lib) = &m.lib {
        let crate_type = if lib.crate_type.is_empty() {
            vec!["lib".to_string()]
        } else {
            lib.crate_type.clone()
        };
        targets.push(serde_json::json!({
            "kind": "lib",
            "name": lib.name,
            "src_path": path_str(&lib.path),
            "crate_type": crate_type,
            "artifact": {
                "debug": path_str(&lib_artifact_path(emit_base, &lib.name, false)),
                "release": path_str(&lib_artifact_path(emit_base, &lib.name, true)),
            },
        }));
    }
    for bin in &m.bins {
        targets.push(serde_json::json!({
            "kind": "bin",
            "name": bin.name,
            "src_path": path_str(&bin.path),
            // The dotted `[[bin]] main` entry (`"xss.it.Main"`), if declared.
            "entry": bin.entry,
            "artifact": {
                "debug": path_str(&bin_artifact_path(emit_base, &bin.name, false)),
                "release": path_str(&bin_artifact_path(emit_base, &bin.name, true)),
            },
        }));
    }

    // ---- dependencies (tagged by source: path / git / registry) ----
    let deps: Vec<serde_json::Value> = m
        .dependencies
        .iter()
        .map(|d| {
            if let Some(path) = &d.path {
                serde_json::json!({ "name": d.name, "source": "path", "path": path_str(path) })
            } else if let Some(git) = &d.git {
                serde_json::json!({
                    "name": d.name,
                    "source": "git",
                    "git": git,
                    "ref": d.git_ref.as_ref().map(GitRef::describe),
                })
            } else {
                serde_json::json!({ "name": d.name, "source": "registry", "version": d.version })
            }
        })
        .collect();

    let profiles: Vec<String> = m.profiles.iter().map(|p| p.name.clone()).collect();

    serde_json::json!({
        "name": m.package.name,
        "manifest_path": path_str(&proot.join("jux.toml")),
        "root": path_str(proot),
        "version": m.package.version,
        "edition": m.package.edition,
        "is_workspace_member": is_member,
        // Default cross-compile triple from `[build] target`, if set.
        "build_target": m.build_target,
        "profiles": profiles,
        "targets": targets,
        "dependencies": deps,
    })
}

/// The EXPECTED built-executable path for a `[[bin]]` target, mirroring the
/// driver's emit convention (`build_emitted_crate`):
/// `<emit_base>/bin-<sanitized>/target/[<triple>/]<profile>/<name><exe>`. The
/// directory segment sanitizes the name (Cargo crate path), but the produced
/// file keeps the raw bin name. The file exists only once that profile/triple
/// has actually been built; the IDE uses this to jump to the artifact.
fn bin_artifact_path(emit_base: &Path, bin_name: &str, release: bool) -> PathBuf {
    let dir = emit_base
        .join(format!("bin-{}", juxc_driver::manifest::default_target_name(bin_name)))
        .join("target");
    artifact_profile_dir(dir, release).join(format!(
        "{bin_name}{}",
        std::env::consts::EXE_SUFFIX,
    ))
}

/// The EXPECTED built-library path for a `[lib]` target (the `rlib` Cargo
/// produces): `<emit_base>/lib-<sanitized>/target/[<triple>/]<profile>/lib<name>.rlib`.
/// `name` has `-` replaced by `_` for the rlib filename (Cargo's convention).
fn lib_artifact_path(emit_base: &Path, lib_name: &str, release: bool) -> PathBuf {
    let dir = emit_base
        .join(format!("lib-{}", juxc_driver::manifest::default_target_name(lib_name)))
        .join("target");
    artifact_profile_dir(dir, release).join(format!("lib{}.rlib", lib_name.replace('-', "_")))
}

/// Append the cross-compile triple segment (when `JUX_TARGET` is set) and the
/// `debug`/`release` profile dir to a crate's `target/` dir — the shared tail of
/// both artifact-path helpers.
fn artifact_profile_dir(target_dir: PathBuf, release: bool) -> PathBuf {
    let mut dir = target_dir;
    if let Ok(triple) = std::env::var("JUX_TARGET") {
        if !triple.is_empty() {
            dir = dir.join(triple);
        }
    }
    dir.join(if release { "release" } else { "debug" })
}

/// Render a path as a string for JSON. Uses the platform-native form (the IDE
/// resolves it the same way it resolves the source paths it already handles).
fn path_str(p: &Path) -> String {
    p.display().to_string()
}

/// What kind of pipeline pass to perform in single-file mode.
#[derive(Debug, Clone, Copy)]
enum Action {
    /// Lex/parse/resolve/typecheck only — no Rust emission.
    Check,
    /// Compile + cargo build, don't execute.
    Build,
    /// Compile + cargo build + execute the emitted binary.
    Run,
}

/// Apply the manifest's `[build] target` (§B.9) as the default
/// cross-compilation triple by setting `JUX_TARGET` — unless the CLI
/// `--target` flag already set it (CLI wins). The driver's cargo invocations
/// read `JUX_TARGET` (see `juxc_driver::cross_target`).
fn apply_default_target(manifest: &juxc_driver::Manifest) {
    if std::env::var_os("JUX_TARGET").is_none() {
        if let Some(triple) = &manifest.build_target {
            std::env::set_var("JUX_TARGET", triple);
        }
    }
}

/// Dispatch table: route to single-file mode if `file` is `Some`, else
/// to the project-mode placeholder. `emit_dir` (if any) overrides the
/// default emit directory in single-file mode.
fn run_single_or_project(
    root: Option<PathBuf>,
    file: Option<PathBuf>,
    action: Action,
    emit_dir: Option<PathBuf>,
    release: bool,
    selection: Selection,
) -> Result<ExitCode> {
    // A file or loose directory has no manifest, so only the built-in
    // profiles are selectable there. A project directory is resolved below.
    let release = match &file {
        Some(path) if !path.join("jux.toml").exists() => match resolve_release_without_manifest(release) {
            Ok(r) => r,
            Err(msg) => {
                eprintln!("jux: {msg}");
                return Ok(ExitCode::from(1));
            }
        },
        _ => release,
    };
    match file {
        // Single-file mode ignores package/target selection (the file is the
        // unit). Warn rather than silently dropping a `-p`/`--bin` the user
        // likely meant for project mode.
        // A DIRECTORY is either a project (it carries a `jux.toml`) or a set
        // of sibling files that make one program. `juxc` has always accepted
        // both; `jux` read it as a file and failed with an OS error, so a
        // multi-file example could not be run by name at all.
        Some(path) if path.is_dir() => {
            if path.join("jux.toml").exists() {
                return run_project(Some(path), action, emit_dir, release, selection);
            }
            run_source_set(&path, action, emit_dir, release)
        }
        Some(path) => {
            if !selection.is_empty() {
                eprintln!(
                    "jux: note: --package/--bin/--lib are ignored when a file argument is given",
                );
            }
            run_single_file(&path, action, emit_dir, release)
        }
        None => run_project(root, action, emit_dir, release, selection),
    }
}

/// `jux build`/`run`/`check` without an explicit file: project
/// mode. Reads `./jux.toml`. When the manifest declares a
/// `[workspace]`, every member is built in dependency order
/// (§B.7). Otherwise the single package's `[lib]`/`[[bin]]`
/// targets are built per the manifest (§B.2). The produced
/// binaries are named from `[[bin]].name`.
fn run_project(
    root: Option<PathBuf>,
    action: Action,
    emit_dir_override: Option<PathBuf>,
    release: bool,
    selection: Selection,
) -> Result<ExitCode> {
    let Some(root_dir) = root else {
        eprintln!(
            "jux: no jux.toml found -- pass a file, run `jux new <name>` first, or use --manifest-path",
        );
        return Ok(ExitCode::from(1));
    };
    let manifest_path = root_dir.join("jux.toml");
    let Some(root_manifest) = juxc_driver::Manifest::load(&root_dir) else {
        eprintln!("jux: failed to load {}", manifest_path.display());
        return Ok(ExitCode::from(1));
    };

    // `[build] optimization` sets the default debug/release build type; an
    // explicit CLI `--release` still wins (§B.9). `[build] target` supplies a
    // default cross-compile triple unless `--target` already set JUX_TARGET.
    let release = match resolve_release(&root_manifest, release) {
        Ok(r) => r,
        Err(msg) => {
            eprintln!("jux: {msg}");
            return Ok(ExitCode::from(1));
        }
    };
    apply_default_target(&root_manifest);

    // Every package (standalone or workspace member) emits under the resolved
    // root's `target/.rust-build`, matching `build_workspace` and the paths
    // `jux metadata` reports, unless `--emit-dir` names another place (a test
    // harness staging many projects keeps their builds out of the sources).
    let emit_root = emit_dir_override.unwrap_or_else(|| root_dir.join("target").join(".rust-build"));
    let is_workspace = !root_manifest.workspace_members.is_empty();

    // ---- Resolve the package to act on ----------------------------------
    let selected: juxc_driver::Manifest = if is_workspace {
        if let Some(pkg) = &selection.package {
            match select_member(&root_manifest, &root_dir, pkg) {
                // The member builds with the root's profiles (§B.9.2).
                Ok((_dir, m)) => juxc_driver::project::with_root_profiles(&m, &root_manifest),
                Err(msg) => {
                    eprintln!("jux: {msg}");
                    return Ok(ExitCode::from(1));
                }
            }
        } else if selection.bin.is_some()
            || selection.lib
            || selection.example.is_some()
            || selection.examples
        {
            // `--bin`/`--lib` need a single package to act on; in a workspace
            // that's ambiguous without `-p`.
            eprintln!(
                "jux: --bin/--lib/--example require selecting a member with --package in a workspace",
            );
            return Ok(ExitCode::from(1));
        } else {
            // Whole-workspace build (existing behavior): build every member.
            return run_workspace(&root_manifest, action, release);
        }
    } else {
        // Standalone single package. A `-p` must name THIS package.
        if let Some(pkg) = &selection.package {
            if !package_name_matches(&root_manifest, pkg) {
                eprintln!(
                    "jux: package `{pkg}` not found (this project is `{}`)",
                    root_manifest.package.name,
                );
                return Ok(ExitCode::from(1));
            }
        }
        root_manifest.clone()
    };

    // `--example` / `--examples` build programs from `examples/` instead of
    // the package's own targets (§B.1.3, §B.15.3).
    if selection.example.is_some() || selection.examples {
        return build_examples(&selected, &emit_root, action, release, &selection);
    }

    if selected.lib.is_none() && selected.bins.is_empty() {
        eprintln!(
            "jux: package `{}` declares no [lib] or [[bin]] target (nothing to do)",
            selected.package.name,
        );
        return Ok(ExitCode::from(1));
    }

    // Validate `--bin`/`--lib` against the FULL package (nice errors) and turn
    // it into a driver `TargetSelection`. The manifest is NOT mutated: the
    // driver consults the complete bin list to exclude sibling entry files.
    let target_sel = match validate_target_selection(&selected, &selection.bin, selection.lib) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("jux: {msg}");
            return Ok(ExitCode::from(1));
        }
    };

    build_and_act(&selected, &emit_root, action, release, &target_sel)
}

/// `--example <name>` / `--examples`: build (and for `run`, execute) programs
/// from the package's `examples/` directory. Each one compiles against the
/// package's library code and dependencies as its own binary (§B.1.3).
fn build_examples(
    manifest: &juxc_driver::Manifest,
    emit_root: &Path,
    action: Action,
    release: bool,
    selection: &Selection,
) -> Result<ExitCode> {
    let all = juxc_driver::project::discover_examples(manifest)?;
    let chosen: Vec<&juxc_driver::project::Example> = match &selection.example {
        Some(name) => match all.iter().find(|e| &e.name == name) {
            Some(e) => vec![e],
            None => {
                let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
                eprintln!(
                    "jux: package `{}` has no example `{name}`; available: {}",
                    manifest.package.name,
                    if names.is_empty() { "(none: examples/ is empty or missing)".to_string() } else { names.join(", ") },
                );
                return Ok(ExitCode::from(1));
            }
        },
        None => all.iter().collect(),
    };
    if chosen.is_empty() {
        eprintln!("jux: package `{}` has no examples", manifest.package.name);
        return Ok(ExitCode::SUCCESS);
    }
    let (dep_sources, _path_deps) = juxc_driver::project::resolve_package_deps(manifest, emit_root)?;
    let facts = juxc_driver::project::cfg_facts_for(manifest, release);
    for example in chosen {
        let build = juxc_driver::project::build_example(
            manifest, &dep_sources, emit_root, release, example, &facts,
        )?;
        print_diagnostics(&build.diagnostics, &build.sources);
        if build.has_errors() {
            eprintln!("jux: example `{}` failed to build", example.name);
            return Ok(ExitCode::from(1));
        }
        let Some(bin) = build.binaries.first() else {
            continue;
        };
        match action {
            Action::Check => {}
            Action::Build => eprintln!("jux: built example `{}` at {}", example.name, bin.binary_path.display()),
            Action::Run => {
                eprintln!("jux: built {}", bin.binary_path.display());
                let status = Command::new(&bin.binary_path)
                    .args(program_args())
                    .status()
                    .with_context(|| format!("running {}", bin.binary_path.display()))?;
                return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
            }
        }
    }
    if matches!(action, Action::Check) {
        eprintln!("jux: check ok");
    }
    Ok(ExitCode::SUCCESS)
}

/// Build one package's (filtered) targets and act on the result: report for
/// `check`/`build`, or run the first produced binary for `run`. Shared by
/// standalone and `-p <member>` project builds.
fn build_and_act(
    manifest: &juxc_driver::Manifest,
    emit_root: &Path,
    action: Action,
    release: bool,
    target_sel: &juxc_driver::project::TargetSelection,
) -> Result<ExitCode> {
    // Resolve `[dependencies]` (path + git, §B.2.2): the dependency's public
    // sources prepend into this package's compilation. The Cargo path-dep
    // lines are DROPPED (the Phase-1 source-inclusion model lowers dep bodies
    // into this package's crate; see `project.rs`).
    let (dep_sources, _path_deps) =
        juxc_driver::project::resolve_package_deps(manifest, emit_root)?;
    let build = juxc_driver::project::build_package_selected(
        manifest,
        &dep_sources,
        &[],
        emit_root,
        release,
        target_sel,
        &juxc_driver::project::cfg_facts_for(manifest, release),
    )?;
    print_diagnostics(&build.diagnostics, &build.sources);
    if build.has_errors() {
        return Ok(ExitCode::from(1));
    }
    match action {
        Action::Check => {
            eprintln!("jux: check ok");
            Ok(ExitCode::SUCCESS)
        }
        Action::Build => {
            if let Some(lib) = &build.library {
                eprintln!("jux: built library crate at {}", lib.crate_dir.display());
            }
            for bin in &build.binaries {
                eprintln!("jux: built {}", bin.binary_path.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Action::Run => {
            let Some(bin) = build.binaries.first() else {
                eprintln!("jux: nothing to run (no [[bin]] target)");
                return Ok(ExitCode::SUCCESS);
            };
            eprintln!("jux: built {}", bin.binary_path.display());
            let status = Command::new(&bin.binary_path)
                    .args(program_args())
                    .status()
                    .with_context(|| {
                format!("running {}", bin.binary_path.display())
            })?;
            let code = status.code().unwrap_or(1) as u8;
            Ok(ExitCode::from(code))
        }
    }
}

/// True when `name` identifies `manifest`'s package: an exact reverse-DNS match
/// (`com.example.app`) or its last segment (`app`).
fn package_name_matches(manifest: &juxc_driver::Manifest, name: &str) -> bool {
    manifest.package.name == name
        || juxc_driver::manifest::default_target_name(&manifest.package.name) == name
}

/// Resolve `-p <name>` to a workspace member: match the member's package name,
/// its last segment, or the member directory's basename. Returns the member's
/// root dir + loaded manifest, or an error listing the available members.
fn select_member(
    root_manifest: &juxc_driver::Manifest,
    root_dir: &Path,
    name: &str,
) -> std::result::Result<(PathBuf, juxc_driver::Manifest), String> {
    let mut available: Vec<String> = Vec::new();
    for rel in &root_manifest.workspace_members {
        let dir = root_dir.join(rel);
        let Some(m) = juxc_driver::Manifest::load(&dir) else {
            continue;
        };
        let basename = Path::new(rel)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(rel.as_str());
        if package_name_matches(&m, name) || basename == name {
            return Ok((dir, m));
        }
        available.push(m.package.name.clone());
    }
    Err(format!(
        "workspace member `{name}` not found; available: {}",
        available.join(", "),
    ))
}

/// Validate a `--bin`/`--lib` request against `manifest` (full target list) and
/// produce the driver [`TargetSelection`]. Errors (with a helpful, available-
/// targets message) when the named target doesn't exist. The manifest is NOT
/// mutated: the driver needs every bin entry to exclude siblings' `main`s.
fn validate_target_selection(
    manifest: &juxc_driver::Manifest,
    bin: &Option<String>,
    lib: bool,
) -> std::result::Result<juxc_driver::project::TargetSelection, String> {
    if lib {
        if manifest.lib.is_none() {
            return Err(format!(
                "package `{}` has no [lib] target",
                manifest.package.name,
            ));
        }
        return Ok(juxc_driver::project::TargetSelection { bin: None, lib_only: true });
    }
    if let Some(name) = bin {
        if !manifest.bins.iter().any(|b| &b.name == name) {
            let names: Vec<&str> = manifest.bins.iter().map(|b| b.name.as_str()).collect();
            return Err(format!(
                "package `{}` has no [[bin]] named `{name}`; available: {}",
                manifest.package.name,
                names.join(", "),
            ));
        }
        return Ok(juxc_driver::project::TargetSelection {
            bin: Some(name.clone()),
            lib_only: false,
        });
    }
    Ok(juxc_driver::project::TargetSelection::default())
}

/// `jux build`/`run`/`check` for a `[workspace]`-root manifest. Builds
/// every member in dependency order via the driver's project
/// orchestration, then (for `run`) executes the first binary of the last
/// member built.
fn run_workspace(
    root: &juxc_driver::Manifest,
    action: Action,
    release: bool,
) -> Result<ExitCode> {
    if matches!(action, Action::Check) {
        // Check still goes through the full build path (compile only,
        // then report). For brevity we reuse build_workspace and just
        // skip running.
    }
    let ws = juxc_driver::project::build_workspace(root, release)?;
    for (name, build) in &ws.members {
        print_diagnostics(&build.diagnostics, &build.sources);
        if build.has_errors() {
            eprintln!("jux: member `{name}` failed to build");
            return Ok(ExitCode::from(1));
        }
        if let Some(lib) = &build.library {
            eprintln!("jux: [{name}] built library crate at {}", lib.crate_dir.display());
        }
        for bin in &build.binaries {
            eprintln!("jux: [{name}] built {}", bin.binary_path.display());
        }
    }
    if matches!(action, Action::Run) {
        // Run the first binary of the last member (the workspace's
        // "application" sits at the top of the topological order).
        if let Some((_, build)) = ws.members.last() {
            if let Some(bin) = build.binaries.first() {
                let status = Command::new(&bin.binary_path)
                    .args(program_args())
                    .status()
                    .with_context(|| {
                    format!("running {}", bin.binary_path.display())
                })?;
                let code = status.code().unwrap_or(1) as u8;
                return Ok(ExitCode::from(code));
            }
        }
        eprintln!("jux: no runnable [[bin]] target in the workspace");
    }
    Ok(ExitCode::SUCCESS)
}

/// `jux test [pattern]` — discover `@Test`-annotated free functions
/// across `src/` and `test/` (plus resolved `[dependencies]` sources,
/// so tests see what the build sees), build a test runner, run it
/// with the filter pattern as argv. Returns the runner's exit code so
/// CI sees test failures (§TS.2/§TS.7/§TS.8).
fn cmd_test(
    root: Option<PathBuf>,
    package: Option<&str>,
    pattern: Option<String>,
    release: bool,
) -> Result<ExitCode> {
    let Some(root_dir) = root else {
        eprintln!(
            "jux: no jux.toml found -- run `jux test` from a project root (or pass --manifest-path)",
        );
        return Ok(ExitCode::from(1));
    };
    let Some(root_manifest) = juxc_driver::Manifest::load(&root_dir) else {
        eprintln!("jux: failed to load {}", root_dir.join("jux.toml").display());
        return Ok(ExitCode::from(1));
    };
    // `-p <member>` in a workspace: test that member's own src/ + test/ dirs.
    // Without `-p`, the root project's sources are tested as before.
    let (cwd, manifest) = if let Some(pkg) = package {
        if root_manifest.workspace_members.is_empty() {
            if !package_name_matches(&root_manifest, pkg) {
                eprintln!(
                    "jux: package `{pkg}` not found (this project is `{}`)",
                    root_manifest.package.name,
                );
                return Ok(ExitCode::from(1));
            }
            (root_dir.clone(), root_manifest)
        } else {
            match select_member(&root_manifest, &root_dir, pkg) {
                Ok((dir, m)) => (dir, juxc_driver::project::with_root_profiles(&m, &root_manifest)),
                Err(msg) => {
                    eprintln!("jux: {msg}");
                    return Ok(ExitCode::from(1));
                }
            }
        }
    } else {
        (root_dir.clone(), root_manifest)
    };
    // `jux test` honors the manifest's default build type too (§B.9); the
    // `assert` builtin stays checked regardless (§TS.2).
    let release = match resolve_release(&manifest, release) {
        Ok(r) => r,
        Err(msg) => {
            eprintln!("jux: {msg}");
            return Ok(ExitCode::from(1));
        }
    };
    apply_default_target(&manifest);
    let binary_name = format!(
        "{}_test",
        manifest
            .package
            .name
            .rsplit('.')
            .next()
            .unwrap_or(manifest.package.name.as_str()),
    );
    let emit_dir = cwd.join("target").join(".rust-build-test");
    // Dependency sources (path + git, §B.2.2): tests compile against
    // the same dependency set the build does.
    let (dep_sources, _path_deps) =
        juxc_driver::project::resolve_package_deps(&manifest, &emit_dir)?;
    let mut sources: Vec<juxc_source::SourceFile> = dep_sources;
    let src_dir = cwd.join("src");
    if src_dir.exists() {
        sources.extend(collect_project_sources(&src_dir)?);
    }
    let test_dir = cwd.join("test");
    if test_dir.exists() {
        sources.extend(collect_project_sources(&test_dir)?);
    }
    if sources.is_empty() {
        eprintln!(
            "jux: no .jux sources under src/ or test/ in {}",
            cwd.display(),
        );
        return Ok(ExitCode::from(1));
    }
    let facts = juxc_driver::project::cfg_facts_for(&manifest, release);
    let result = juxc_driver::compile_workspace_test_cfg(sources, &facts)?;
    print_diagnostics(&result.diagnostics, &result.sources);
    let any_error = result
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    if any_error {
        return Ok(ExitCode::from(1));
    }
    let Some(crate_) = result.crate_ else {
        eprintln!("jux: nothing to test");
        return Ok(ExitCode::SUCCESS);
    };
    // A named `--profile` must be defined in the runner's emitted Cargo.toml,
    // so the manifest (which carries the `[profile.*]` tables) is woven in
    // then. Without one the runner keeps its plain manifest as before.
    let with_profiles = selected_profile().is_some().then_some(&manifest);
    let artifact =
        juxc_driver::build_with_manifest(&crate_, &emit_dir, &binary_name, release, with_profiles)?;
    // Run the test binary, inherit stdio so the user sees PASS/FAIL
    // output in real time. Forward the filter pattern as argv and
    // the exit code so CI gates work.
    let status = Command::new(&artifact.binary_path)
        .args(pattern.iter())
        .status()
        .with_context(|| format!("running {}", artifact.binary_path.display()))?;
    let code = status.code().unwrap_or(1) as u8;
    Ok(ExitCode::from(code))
}

/// Walk `src_dir` recursively and collect every `.jux` file as a
/// loaded `SourceFile`. Sort order is path-lexicographic so
/// diagnostics line up across runs.
fn collect_project_sources(src_dir: &Path) -> Result<Vec<juxc_source::SourceFile>> {
    let mut paths: Vec<PathBuf> = Vec::new();
    walk_jux_files(src_dir, &mut paths)?;
    paths.sort();
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let contents = std::fs::read_to_string(&p)
            .with_context(|| format!("reading {}", p.display()))?;
        out.push(juxc_source::SourceFile::new(p, contents));
    }
    Ok(out)
}

fn walk_jux_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading dir {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_jux_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jux") {
            out.push(path);
        }
    }
    Ok(())
}

/// Compile one `.jux` file through the driver and (optionally) build/run
/// the emitted Rust crate.
///
/// The exit code returned reflects what the user cares about:
/// - Non-zero from juxc diagnostics → 1.
/// - Non-zero from the emitted binary (`Run` only) → forwarded as-is.
/// - Everything else → 0.
/// Compile every `.jux` under `dir` as ONE program.
///
/// This is the loose multi-file shape - sibling files that import each other
/// but carry no manifest, which is most of `examples/`. `target/` and hidden
/// directories are skipped, the same rule `juxc` walks by.
fn run_source_set(
    dir: &Path,
    action: Action,
    emit_dir_override: Option<PathBuf>,
    release: bool,
) -> Result<ExitCode> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_jux_files(dir, &mut files)?;
    files.sort();
    if files.is_empty() {
        eprintln!("jux: no .jux files under {}", dir.display());
        return Ok(ExitCode::from(1));
    }
    let mut sources = Vec::with_capacity(files.len());
    for f in &files {
        let contents = std::fs::read_to_string(f)
            .with_context(|| format!("reading {}", f.display()))?;
        sources.push(juxc_source::SourceFile::new(f.clone(), contents));
    }
    let facts = juxc_driver::CfgFacts::new(release, juxc_driver::Profile::Full);
    let result = juxc_driver::compile_workspace_cfg(sources, &facts)?;
    finish_compile(result, dir, action, emit_dir_override, release)
}

/// Every `.jux` under `dir`, skipping `target/` and hidden directories.
fn collect_jux_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            collect_jux_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jux") {
            out.push(path);
        }
    }
    Ok(())
}

fn run_single_file(
    input: &Path,
    action: Action,
    emit_dir_override: Option<PathBuf>,
    release: bool,
) -> Result<ExitCode> {
    // Phase 1–N: lex/parse/resolve/typecheck/lower via the shared driver.
    let contents = std::fs::read_to_string(input)
        .with_context(|| format!("reading {}", input.display()))?;
    let source = juxc_source::SourceFile::new(input.to_path_buf(), contents);
    let facts = juxc_driver::CfgFacts::new(release, juxc_driver::Profile::Full);
    let result = juxc_driver::compile_workspace_cfg(vec![source], &facts)?;
    finish_compile(result, input, action, emit_dir_override, release)
}

/// Report, then build and possibly run - shared by the single-file and
/// source-set entry points, which differ only in how they gather their input.
fn finish_compile(
    result: juxc_driver::CompileResult,
    input: &Path,
    action: Action,
    emit_dir_override: Option<PathBuf>,
    release: bool,
) -> Result<ExitCode> {

    print_diagnostics(&result.diagnostics, &result.sources);

    let any_error = result.diagnostics.iter().any(|d| matches!(d.severity, Severity::Error));
    if any_error {
        return Ok(ExitCode::from(1));
    }

    match action {
        Action::Check => {
            // No errors and we don't need a binary: the user got what they asked for.
            eprintln!("jux: check ok");
            Ok(ExitCode::SUCCESS)
        }
        Action::Build | Action::Run => {
            // We need the emitted crate. The check path skips lowering when
            // there are errors, but here we expect Some(crate_).
            let Some(crate_) = result.crate_ else {
                // No errors but no crate — only happens for an empty compilation unit.
                eprintln!("jux: nothing to build");
                return Ok(ExitCode::SUCCESS);
            };

            // Caller-provided emit dir wins; otherwise fall back to
            // `<input parent>/target/.rust-build/`, matching juxc's default.
            let emit_dir = emit_dir_override.unwrap_or_else(|| default_emit_dir(input));
            // Name the emitted crate after its input, the same way `juxc`
            // does. Every single-file program used to emit `jux_emitted`, so
            // two of them could not share a `CARGO_TARGET_DIR` without
            // overwriting each other's binary - which is what made building
            // the whole example corpus cost a full dependency tree per file.
            let artifact = juxc_driver::build(
                &crate_,
                &emit_dir,
                &juxc_driver::crate_name_for_input(input),
                release,
            )?;
            eprintln!("jux: built {}", artifact.binary_path.display());

            if matches!(action, Action::Run) {
                // Spawn the emitted binary with inherited stdio so the
                // user sees its output verbatim. Forward the exit code so
                // CI / IDEs see a non-zero exit when the user's program
                // exits non-zero.
                let status = Command::new(&artifact.binary_path)
                    .args(program_args())
                    .status()
                    .with_context(|| {
                    format!("running {}", artifact.binary_path.display())
                })?;
                let code = status.code().unwrap_or(1) as u8;
                Ok(ExitCode::from(code))
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
    }
}

/// `<input parent>/target/.rust-build/` — same default `juxc` uses.
fn default_emit_dir(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    parent.join("target").join(".rust-build")
}

/// The diagnostic format and color chosen on the command line.
static DIAGNOSTIC_STYLE: std::sync::OnceLock<(juxc_driver::render::DiagnosticFormat, bool)> =
    std::sync::OnceLock::new();

/// Decide the diagnostic format and color once, from `--diagnostic-format`
/// and `--color` (and whether stderr is a terminal).
fn set_diagnostic_style(format: Option<&str>, color: &str) -> std::result::Result<(), String> {
    use std::io::IsTerminal;
    let terminal = std::io::stderr().is_terminal();
    let format = match format {
        Some(f) => juxc_driver::render::DiagnosticFormat::parse(f)
            .ok_or_else(|| format!("unknown --diagnostic-format `{f}` (human, compact, short, line, json)"))?,
        None => juxc_driver::render::DiagnosticFormat::default_for(terminal),
    };
    let color = juxc_driver::render::ColorChoice::parse(color)
        .ok_or_else(|| format!("unknown --color `{color}` (auto, always, never)"))?
        .enabled(terminal);
    let _ = DIAGNOSTIC_STYLE.set((format, color));
    Ok(())
}

/// Print diagnostics in the chosen format, in source order and de-duplicated
/// exactly as `juxc` shows them: text on stderr, JSON on stdout (§D.2).
fn print_diagnostics(diagnostics: &[Diagnostic], sources: &[juxc_source::SourceFile]) {
    let (format, color) = DIAGNOSTIC_STYLE
        .get()
        .copied()
        .unwrap_or((juxc_driver::render::DiagnosticFormat::Line, false));
    if format == juxc_driver::render::DiagnosticFormat::Json {
        print!("{}", juxc_driver::render::render_json(diagnostics, sources, 0));
    } else {
        eprint!("{}", juxc_driver::render::render_text(diagnostics, sources, format, color));
    }
}

/// Print a "not yet implemented" message pointing at the relevant spec
/// section, then exit with status 2 so scripts can detect it. Retained as
/// scaffolding for CLI subcommands still on the roadmap (§B.15).
#[allow(dead_code)]
fn not_yet_implemented<S: Into<String>>(what: S) -> Result<ExitCode> {
    eprintln!("jux: {} is not yet implemented", what.into());
    eprintln!("     (see JUX-BUILD-SYSTEM-ADDENDUM.md §B.15 for the full CLI surface)");
    Ok(ExitCode::from(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// RAII temp dir (std-only) for the project-resolution / selection tests.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let id = N.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("jux-cli-test-{}-{}", std::process::id(), id));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `--manifest-path <dir>` resolves to that directory verbatim.
    #[test]
    fn resolve_root_explicit_dir() {
        let dir = TempDir::new();
        let got = resolve_project_root(Some(dir.path())).unwrap();
        assert_eq!(got, dir.path());
    }

    /// `--manifest-path <dir>/jux.toml` resolves to the file's parent directory.
    #[test]
    fn resolve_root_explicit_file_uses_parent() {
        let dir = TempDir::new();
        let file = dir.path().join("jux.toml");
        std::fs::write(&file, "[package]\nname = \"app\"\n").unwrap();
        let got = resolve_project_root(Some(&file)).unwrap();
        assert_eq!(got, dir.path());
    }

    /// A bare `--manifest-path jux.toml` (no parent component) resolves to cwd.
    #[test]
    fn resolve_root_bare_filename_is_cwd() {
        let got = resolve_project_root(Some(Path::new("jux.toml"))).unwrap();
        assert_eq!(got, PathBuf::from("."));
    }

    /// Load a `Manifest` from a written `jux.toml` for the selection tests.
    fn load_manifest(toml: &str) -> (juxc_driver::Manifest, TempDir) {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("jux.toml"), toml).unwrap();
        let m = juxc_driver::Manifest::load(dir.path()).expect("manifest loads");
        (m, dir)
    }

    /// `package_name_matches` accepts the full reverse-DNS name and its last
    /// segment, but nothing else.
    #[test]
    fn package_name_matches_full_and_segment() {
        let (m, _d) = load_manifest("[package]\nname = \"com.example.app\"\n");
        assert!(package_name_matches(&m, "com.example.app"));
        assert!(package_name_matches(&m, "app"));
        assert!(!package_name_matches(&m, "other"));
    }

    /// `--bin <name>` validates against the package and yields a bin selection;
    /// an unknown bin is a clear error.
    #[test]
    fn validate_selection_bin() {
        let (m, _d) = load_manifest(
            "[package]\nname = \"app\"\n\n[[bin]]\nname = \"a\"\npath = \"src/a.jux\"\n",
        );
        let sel = validate_target_selection(&m, &Some("a".to_string()), false).unwrap();
        assert_eq!(sel.bin.as_deref(), Some("a"));
        assert!(!sel.lib_only);
        assert!(validate_target_selection(&m, &Some("ghost".to_string()), false).is_err());
    }

    /// `--lib` requires a `[lib]` target; without one it errors, with one it
    /// yields a lib-only selection.
    #[test]
    fn validate_selection_lib() {
        let (no_lib, _d1) =
            load_manifest("[package]\nname = \"app\"\n\n[[bin]]\nname = \"a\"\npath = \"src/a.jux\"\n");
        assert!(validate_target_selection(&no_lib, &None, true).is_err());

        let (with_lib, _d2) =
            load_manifest("[package]\nname = \"l\"\n\n[lib]\nname = \"l\"\npath = \"src/lib.jux\"\n");
        let sel = validate_target_selection(&with_lib, &None, true).unwrap();
        assert!(sel.lib_only);
        assert!(sel.bin.is_none());
    }

    /// The bin artifact path matches the driver's emit convention:
    /// `<base>/bin-<name>/target/<profile>/<name><exe>`.
    #[test]
    fn bin_artifact_path_shape() {
        let base = Path::new("/proj/target/.rust-build");
        let p = bin_artifact_path(base, "app", false);
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.contains("/bin-app/target/debug/app"), "got: {s}");
        assert!(s.ends_with(std::env::consts::EXE_SUFFIX) || std::env::consts::EXE_SUFFIX.is_empty());
    }
}
