# Jux — Language, Compiler, and Project Tool

This repository holds both the **Jux programming language specification** and
its **reference implementation**. The spec is the contract; the implementation
is what we're building against it.

- **Specification:** [`Architecture/`](Architecture/) — `JUX-LANG-V1.md` plus
  16 normative addenda covering grammar, semantics, type system, ABI,
  diagnostics, build system, async, exceptions, and more.
- **Implementation:** this directory — a Cargo workspace producing the `juxc`
  compiler binary and the `jux` project tool.

## Status

**Pre-bootstrap.** No working compiler yet. Lexer is in (`crates/juxc-lex/`,
32 unit tests green); parser is next.

The first milestone is end-to-end "Hello, world!" — `jux run examples/hello.jux`
should print `Hello, world!`. The implementation strategy is **Phase 1 of the
language plan** ([`Architecture/JUX-LANG-V1.md`](Architecture/JUX-LANG-V1.md)
§2.2): the compiler transpiles `.jux` source to idiomatic Rust source, then
invokes `cargo`/`rustc` to produce the native binary.

## Layout

```
juxlang/
├── Architecture/              # the language specification
│   ├── JUX-LANG-V1.md         # main dossier
│   ├── JUX-*-ADDENDUM.md      # 16 normative addenda
│   ├── JUX-GAPS-ROADMAP.md
│   └── Docs/                  # rendered HTML view of the spec (build.ps1)
│
├── Cargo.toml                 # Cargo workspace root
├── rust-toolchain.toml        # stable rustc
├── examples/
│   └── hello.jux              # the milestone-1 target
├── crates/
│   ├── juxc-source/           # source files, spans, positions (shared)
│   ├── juxc-diagnostics/      # diagnostic types, E-codes, JSON output (shared)
│   ├── juxc-lex/              # Phase 1: lexer (per pipeline §C.2.1)
│   ├── juxc-ast/              # Phase 3: AST types (per grammar §A.2)
│   ├── juxc-parse/            # Phase 2: parser (per pipeline §C.2.2)
│   ├── juxc-resolve/          # Phase 4: name resolution (per pipeline §C.2.4)
│   ├── juxc-tycheck/          # Phases 6–9: type checking (per pipeline §C.3)
│   ├── juxc-backend-rust/     # Phase 19: lowering to Rust source (per pipeline §C.9)
│   └── juxc-driver/           # phase orchestration
└── bin/
    ├── juxc/                  # the compiler binary
    └── jux/                   # the project tool (cargo-equivalent)
```

The crate names match the module names called out in the **Compiler Pipeline
Addendum** §C.1.2. As phases are added (MIR build, borrow inference, monomorph,
DCE, etc.), they get their own crate under the same naming scheme.

## The two binaries

Per the **Build System Addendum** §B.11:

- **`juxc`** is the compiler. Operates on individual files or modules. Doesn't
  read `jux.toml` or resolve dependencies.
- **`jux`** is the project tool. Reads `jux.toml`, resolves dependencies, and
  dispatches `juxc` invocations. The `jux build` / `jux run` / `jux test` /
  `jux new` commands live here.

Day-to-day use is `jux`. `juxc` is invoked by `jux`, by the language server,
and by integration with foreign build systems (Bazel, Buck, etc.).

## Building

```sh
cargo check               # verify the workspace compiles clean
cargo build               # build everything
cargo run --bin jux -- --help
```

## Implementation discipline

The spec is **authoritative**. Every behavior — keywords, error codes,
operator precedence, file layouts, default values — must trace to a clause in
`Architecture/JUX-LANG-V1.md` or an addendum. If a question doesn't have a
spec answer, update the spec **before** implementing.

## License

Apache-2.0.
