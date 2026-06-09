# Jux — Language, Compiler, and Project Tool

This repository holds both the **Jux programming language specification** and
its **reference implementation**. The spec is the contract; the implementation
is what we're building against it.

- **Specification:** [`Architecture/`](Architecture/) —
  [`JUX-LANG-V1.md`](Architecture/JUX-LANG-V1.md) plus 20+ normative addenda
  covering grammar, semantics, type system, ABI, diagnostics, build system,
  async, exceptions, annotations, class representation, and more.
- **Implementation:** this directory — a Cargo workspace producing the `juxc`
  compiler binary and the `jux` project tool.

## Status

**Working compiler + project builds + editor tooling.** The pipeline runs
end-to-end: lex → parse → resolve → typecheck → lower-to-Rust → `cargo build` →
run. `jux run examples/hello.jux` prints `Hello, world!`, and
`juxc <project-dir> --run` builds a whole multi-package source tree. The Phase 1
strategy ([`JUX-LANG-V1.md`](Architecture/JUX-LANG-V1.md) §2.2) is in place:
`juxc` transpiles `.jux` source to idiomatic, human-readable Rust, then invokes
`cargo`/`rustc` to produce the native binary.

### Rust-std / crate interop (the std is Rust's std)

Jux uses the **Rust standard library (and any Rust crate) as its own std**,
surfaced in Jux syntax:

- **Autocomplete / hover / goto-definition** over Rust `std` and project crates,
  rendered in Jux syntax — generated on demand from the installed toolchain's
  rustdoc JSON (`juxc-bindgen`), never hand-curated. The editor reaches *into*
  the generated declaration stubs.
- **Compile against it.** `import rust.std.PathBuf; var p = new PathBuf();
  p.reserve(16);` lowers to the real `std::path::PathBuf` — real-path imports,
  `new X()` → `X::new()`, camelCase → snake_case methods, `&mut self`-aware
  bindings (`JUX-BINDGEN-ADDENDUM.md` §G.9.2, in progress).
- A hand-written `jux.std` (Java-shaped `List`/`Map`/`String`/exceptions) is
  still the default std today; `rust.std` is the future replacement it's being
  migrated toward.

### Editor tooling

- **`juxc-lsp`** — a language server (the single semantic source of truth):
  workspace-wide diagnostics, hover signatures + docs, receiver-aware
  completion, auto-import code actions, goto-definition, and document symbols.
- **IntelliJ plugin** (`ide/intellij-plugin/`) — a native Java-style PSI plugin;
  `./gradlew buildPlugin` produces an installable `.zip`.

### Implemented today

Roughly, anything you find in [`examples/`](examples/) compiles and runs.
That includes:

- **Free functions** with primitive types, control flow, arithmetic, bitops,
  ranges, casts (`abs.jux`, `arithmetic.jux`, `bitops.jux`, `casts.jux`,
  `loop_range.jux`).
- **Classes** — fields, constructors, methods, `static`/`final`,
  visibility, **bare instance-field access** (`f` ≡ `this.f`), `final`/`const`
  method parameters, and C#-style **properties** (`{ get; set; }`,
  expression-bodied with `->`) (`encapsulation.jux`, `point.jux`, `vector3.jux`).
- **`struct`** declarations and **generic enums** (`enum Cow<B>`); **nested
  generics** (`List<List<int>>`), function types, and tuple/pointer placeholders.
- **Inheritance** — `extends`, `super(...)`, overrides, abstract classes,
  `sealed`/`non-sealed` (`animals.jux`, `shapes.jux`, `sealed_shapes.jux`).
- **Interfaces** — default methods, static methods, constants
  (`interface_constants.jux`, `interface_static.jux`).
- **Enums and `match`** with exhaustiveness checking and payload binding
  (`colors_enum.jux`, `colors_match.jux`, `match_payload.jux`,
  `op_enum.jux`).
- **Generics** — `class A<T>`, bounded type parameters, wildcards
  (`box_generic.jux`, `bounded_generic.jux`, `extends_generic.jux`,
  `wildcards.jux`).
- **Records** with auto-derived display/methods (`record_display.jux`,
  `record_methods.jux`).
- **Lambdas and method references** (`lambdas.jux`, `method_ref.jux`,
  `higher_order.jux`).
- **Operator overloading** (`op_overload.jux`, `op_arithmetic.jux`,
  `op_cmp.jux`).
- **Annotations** — built-in lookups are case-insensitive
  (`annotations.jux`).
- **String interpolation** — `$"hello ${name}"` syntax (`greet.jux`,
  `names.jux`).
- **Multi-file workspaces** — cross-file `import`s, package-private
  visibility (`examples/multifile/`, `examples/showcase/`).

### Stubbed / not yet implemented

- **Project mode works**: `juxc <dir> --run` builds a multi-package source
  tree, and `jux.toml` (`[package]`/`[lib]`/`[[bin]]`/`[dependencies]`/
  `[workspace]`) drives multi-module builds with per-module binary metadata
  (version/author/icon). `jux new` / `jux test` remain stubs.
- **`rust.std` compile coverage is partial** — construction + method calls work;
  generics-on-construction, free functions, traits/operators, and the full
  type-mapping are in progress (§G.9.2). Real `tuple-type`/`pointer-type` syntax
  is surfaced as `Tuple<…>`/`Ptr<…>` placeholders pending the unsafe/C-interop
  work.
- `native` and `synchronized` method modifiers are intentionally out of
  scope (Jux is not Java — JNI bridging and intrinsic monitors will be
  redesigned for the Rust backend).

## Layout

```
juxlang/
├── Architecture/                # the language specification
│   ├── JUX-LANG-V1.md           # consolidated dossier
│   ├── JUX-*-ADDENDUM.md        # 20+ normative addenda
│   ├── JUX-GAPS-ROADMAP.md
│   └── Docs/                    # rendered HTML view of the spec (build.ps1)
│
├── Cargo.toml                   # Cargo workspace root
├── rust-toolchain.toml          # stable rustc
├── examples/                    # .jux programs (every one compiles & runs)
│   ├── hello.jux                # the milestone-1 target
│   ├── animals.jux              # inheritance + abstract methods
│   ├── colors_match.jux         # enums + match exhaustiveness
│   ├── box_generic.jux          # generics
│   ├── multifile/               # cross-file imports + package visibility
│   └── showcase/                # multi-module sample
├── crates/
│   ├── juxc-source/             # source files, spans, positions (shared)
│   ├── juxc-diagnostics/        # diagnostic types, E-codes, JSON output
│   ├── juxc-lex/                # Phase 1: lexer (pipeline §C.2.1)
│   ├── juxc-ast/                # Phase 3: AST types (grammar §A.2)
│   ├── juxc-parse/              # Phase 2: parser (pipeline §C.2.2)
│   ├── juxc-resolve/            # Phase 4: name resolution (pipeline §C.2.4)
│   ├── juxc-tycheck/            # Phases 6–9: type checking (pipeline §C.3)
│   ├── juxc-backend-rust/       # Phase 19: lowering to Rust (pipeline §C.9)
│   ├── juxc-bindgen/            # rustdoc JSON → Jux-syntax .jux.d stubs (§G)
│   ├── juxc-lsp/                # language server (LSP; §L)
│   └── juxc-driver/             # phase orchestration + project/workspace builds
├── ide/
│   └── intellij-plugin/         # IntelliJ plugin (Java-style PSI; ./gradlew buildPlugin)
└── bin/
    ├── juxc/                    # the compiler binary
    └── jux/                     # the project tool (cargo-equivalent)
```

Crate names match the modules called out in the **Compiler Pipeline
Addendum** §C.1.2. Future phases (MIR build, borrow inference, monomorph,
DCE, …) get their own crate under the same naming scheme.

## The two binaries

Per the **Build System Addendum** §B.11:

- **`juxc`** is the compiler. Operates on individual files or directories.
  Doesn't read `jux.toml` or resolve dependencies.
- **`jux`** is the project tool. Reads `jux.toml`, resolves dependencies,
  and dispatches `juxc` invocations. The `jux build` / `jux run` /
  `jux test` / `jux new` commands live here.

Day-to-day use is `jux`. `juxc` is invoked by `jux`, by the language
server, and by foreign build systems (Bazel, Buck, etc.).

## Step 1 — Build the compiler

This is a Rust workspace, so the compiler itself is built with `cargo`.
You only do this once (or after changing compiler code).

```sh
cargo check                  # verify the workspace compiles clean
cargo build                  # build everything (debug → target/debug/)
cargo build --release        # optimized juxc + jux → target/release/
```

After `cargo build --release` you get two binaries:

- `target/release/juxc.exe` — the file-level compiler
- `target/release/jux.exe`  — the project tool

Copy them to the repo root (or anywhere on your `PATH`) so you can invoke
them as `./juxc.exe` / `./jux.exe` in Step 2:

```sh
copy target\release\juxc.exe .
copy target\release\jux.exe  .

./juxc.exe --help
./jux.exe  --help
```

## Step 2 — Build and run a Jux program

This step uses the binaries produced by Step 1 — **no `cargo` involved
on the command line**. (Internally `juxc` does invoke `cargo build` on
the Rust crate it emits, but that's an implementation detail.)

There are **two independent profiles** to keep in mind:

1. **The compiler's own profile** — set in Step 1 (`cargo build` vs
   `cargo build --release` decides whether `juxc` itself is optimized).
2. **The emitted program's profile** — set here with the `--release`
   flag on `juxc` / `jux`. Without it, the emitted program lands in
   `<emit-dir>/target/debug/<Name>.exe`. With it,
   `<emit-dir>/target/release/<Name>.exe`. `--emit-dir` overrides the
   default emit location.

### Using `juxc` (the compiler)

`juxc` operates on individual files or directories. It exposes
`--name` (override the produced binary's name) and `--release`
(build the emitted program optimized).

```sh
# Lower to Rust only — no cargo build, no execute.
./juxc.exe examples/hello.jux

# Compile + cargo build + execute. Produces hello.exe (file-stem default).
./juxc.exe --run examples/hello.jux

# Custom binary name — produces Hello.exe instead of hello.exe.
./juxc.exe --name Hello --run examples/hello.jux

# Build only (no execute), custom name.
./juxc.exe --name Hello --build examples/hello.jux

# Release-mode emitted program. Lands in target/.rust-build/target/release/.
./juxc.exe --name Hello --release --run examples/hello.jux
```

`--name` defaults to the input's file-stem (single file) or directory
name (folder input). The name flows into the emitted `Cargo.toml`'s
`[[bin]]` entry and drives the lookup of the resulting executable.

### Using `jux` (the project tool)

`jux` is the cargo-equivalent project tool. It currently does **not**
expose `--name` — per spec, the package name will come from `jux.toml`
once project mode lands. It does forward `--release` to the inner
`cargo build`.

```sh
# Compile + cargo build + execute. Forwards stdout/stderr and exit code.
./jux.exe run examples/hello.jux

# Compile + cargo build, but don't run.
./jux.exe build examples/hello.jux

# Type-check only (no Rust emission, no cargo invocation).
./jux.exe check examples/hello.jux

# Release-mode emitted program.
./jux.exe run --release examples/hello.jux
./jux.exe build --release examples/hello.jux --emit-dir /tmp/hello
```

### Compiling a multi-file project

Point either binary at a directory; every `.jux` file inside is compiled
together as a single workspace (cross-file `import`s resolve,
package-private visibility is enforced across the unit boundary):

```sh
# via juxc directly (workspace mode)
./juxc.exe --run examples/multifile

# or list specific files
./juxc.exe --run examples/showcase/app.jux examples/showcase/math.jux examples/showcase/shapes.jux
```

### What `jux run` does under the hood

1. **Driver** (`juxc-driver`) runs lex → parse → resolve → typecheck.
2. **Backend** (`juxc-backend-rust`) lowers the AST to an idiomatic Rust
   crate written to `<input-parent>/target/.rust-build/` (override with
   `--emit-dir`).
3. **`cargo build`** is invoked on that emitted crate.
4. The produced binary is executed with inherited stdio; its exit code is
   forwarded.

If any phase produces an error-severity diagnostic, the pipeline stops and
`jux` exits 1.

## Implementation discipline

The spec is **authoritative**. Every behavior — keywords, error codes,
operator precedence, file layouts, default values — must trace to a clause
in `Architecture/JUX-LANG-V1.md` or an addendum. If a question doesn't have
a spec answer, update the spec **before** implementing.

## License

Apache-2.0.
