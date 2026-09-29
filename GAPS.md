# Jux: open gaps

Written 2026-09-25. Everything here was measured against a built toolchain, not
inferred from reading code. Where a claim came from an audit rather than my own
probe, it says so.

Branch state at the time of writing: `gaps` = `820426d`, pushed, gate green
(1544 compiler tests passed / 0 failed, clippy clean, Java twins
`match=120 mismatch=0`, plugin 1132 tests / 0 failures).

---

## 1. Where the project stands

Last measured completeness: **76%** on 2026-09-24 (unweighted mean of five
areas; the weighted total reads 79% but core language is both the largest and
the strongest area, so 76% is the fair number). That figure came from ~790
probe programs across all 30 `Architecture/*.md` documents and predates
everything fixed on 2026-09-25, so it is now low. It should be re-measured the
same way rather than estimated.

| Area | 2026-09-15 | 2026-09-24 |
|---|---|---|
| Core language | 74% | 85% |
| Object model | 77% | 71% |
| Runtime and library | 72% | 70% |
| Systems and build | 71% | 76% |
| Tooling | 65% | 78% |

Object model and runtime went DOWN because the earlier pass over-credited them,
not because anything regressed.

---

## 2. Closed on 2026-09-25

Recorded because the ERRATA numbers are the entry point for anyone picking this
up, and because several of these have documented boundaries that are still open
(section 4).

| ERRATA | What it closed |
|---|---|
| E96 | A user type may be named `T`, `String`, `Vec`, `Exception`, `Iterator`. Bare-name resolution became unit-scoped; `jux.std` and the `rust.*` stubs are a library realm that never binds a bare name to a user declaration. New §M.16. |
| E97 | A Rust type that is not `Clone + Debug` can be held in an aggregate. Bindgen now discovers `@RustDebug`, `@RustPartialEq`, `@RustDefault` beside `@RustClone`; `__jux_show!` gained a bottom tier that prints `<TypeName>`. |
| E98 | A function type over a polymorphic class lowers to the handle, so callbacks over a class hierarchy work. |
| E99 | Part 2 closed later the same day by E107 (gap 1). |
| E100 | Members reached through a wildcard over a class bound: field reads, method calls, and `? super` printing through `Display`. |
| E101 | A sealed class hierarchy is a reference hierarchy, not a Rust enum. The enum lowering is deleted. `case Sub(part, ...)` got a real lowering and a grammar production. |
| E102 | An aliased or fully-qualified library type is not shadowed by a same-named user class. |
| E103 | A `[[bin]]` entry file is not a member of the package tree: `[lib]` plus several `[[bin]]` with `src/bin/` entries builds. |
| E104 | Annotations on parameters and locals parse and are target-checked (`E0470`). |
| E105 | `@entry` selects the entry point, with `symbol=`, `E0320`/`E0321`/`E0322`/`E0324`. |
| E95 | The for-each binder takes `final` / `const`, enforced as `E0464`. |
| E94 | `parallel(items, f)` fans out over its collection; `Task.all` / `allSettled` hand back a collection handle. |
| E87 | A spawned task is concurrent, not parallel, and carries no `Send` bound, so `spawn` can carry a class. |
| E90-E93 | `ref` aliases a plain local; an observer parameter attaches; `$name` keeps a double's point; `cancel()` runs `finally`; a static-init cycle is `E0497` rather than a hang. |

---

## 3. Open gaps, by relevance

### Blocks ordinary use

**1. CLOSED 2026-09-25 (`dc929a7`, ERRATA E107).** ~~`Box<int?>` leaks `Option<isize>: Display`.~~ Contradicts §T.2.1's explicit
promise that a compiler-added bound can never make a legal type argument
illegal. Recorded OPEN as ERRATA **E99**, which also records the trap: removing
the `Display` bound looks right and is wrong, because the spez probe resolves
ONCE in the generic body against the parameter's declared bounds, so with only
`T: Debug` in scope every instantiation takes the Debug arm.
`examples/generic_declarations_print.jux` silently changed from
`wrap(Cell(value: in))` to `wrap(Cell { value: "in" })` when it was tried. The
route E99 names: make `Debug` canonical for Jux values, emitting a hand-written
`Debug` on every class, record and enum that prints the type's string form
(§O.7.1). E97 already has a worked example of writing that impl rather than
deriving it, in `decls/classes.rs`.

**2. CLOSED 2026-09-26 (E118, E120).** Every generic class, interface, record and enum moves `Clone + Debug` to the members that copy a `T`; a parameter keeps it on the whole declaration only for a stated rule, and a use the argument cannot meet is the checker's `E0457`, not rustc's. What stays out is listed in the entry's known boundary (a record or enum VALUE over a non-`Clone` type is itself not copyable, as E97's non-generic record is). ~~`Cell<File>` over a Jux generic.~~ The blanket `Clone + Debug + 'static`
on every generic declaration. Dropping it wholesale breaks the core library
(`Iterable.reduce<U>` clones its accumulator; `LazyIterable.chain` needs
`T: Clone`); dropping it only from the struct emitter leaves the inherent
`impl` bounded, so `Cell::<File>::new` stays unreachable. Needs per-parameter
bound inference in the style of the existing `hash_key_params` /
`eq_bound_params` passes. `Vec<File>` works, since a foreign generic carries
its own impls.

**3. CLOSED 2026-09-25 (`770de4b`, ERRATA E106).** ~~`jux.toml` is effectively unvalidated.~~ `name = "MyApp"`, a missing
`version`, a missing `edition`, `edition = "2015"` and unknown tables all pass
`jux check`. Malformed TOML is an `eprintln!` warning and a `None` return
(`crates/juxc-driver/src/manifest.rs:470`), so a caller cannot tell "no
manifest" from "bad manifest". §B.2.2 marks all three keys REQUIRED, pins the
name to `^[a-z][a-z0-9]*(\.[a-z][a-z0-9_]*)+$`, and allows only edition
`"2026"`. No manifest diagnostic code exists; one has to be allocated.

**4. CLOSED 2026-09-25 (`770de4b`, ERRATA E106).** ~~A dependency's entry files leak into the dependent.~~
`collect_dependency_sources` (`crates/juxc-driver/src/project.rs:836`) loads a
dependency's whole `src/` tree including entry files, so a package that
path-depends on a package having any `[[bin]]` gets the dependency's `main` and
fails with `E0400`. The one-call fix is NOT safe: `cmd_doc` and `cmd_doctest`
call the same function on their own package, so filtering would silently drop
`src/main.jux` items from generated docs. The two uses must be separated first.
`load_src_tree_without_entries(manifest, keep)` from the E103 work is the filter
to reuse.

**5. CLOSED 2026-09-25 (`8c47abb`, under ERRATA E102).** ~~Index and `@RustRefOut` reads through an alias import.~~ Same family as
E102 but a different root cause: these fail with NO user class present.
```jux
import rust.std.Vec as RVec;
RVec<int> v = new RVec<int>(); v.push(5); print($"${v[0]}");
```
gives `error[E0608]: cannot index into a value of type Rc<JuxCell<..>>`; the
emitted line is `v[0]` with no `.borrow()`. Spelled `Vec<int>` or
`rust.std.Vec<int>` it emits `v.borrow()[0]` correctly. Likewise
`d.front() ?? 0` through an alias does not get its `.cloned()` and gives
`E0308`. Almost certainly two more sites that did not get E102's memo
("a resolved name keeps its package"), not a new rule.

**6. CLOSED 2026-09-26 (phase 2 merge, ERRATA E111).** ~~`rust.std.File.open("x")` as a fully-qualified static call~~ emitted the raw
`Result` into a `rust.std.File?` slot (`E0308`). The imported spelling
`File.open(..)` is correct.

### Missing surface

**7. CLOSED 2026-09-26 (phase 2 merge, ERRATA E114).** ~~`Task.completed`, `Task.failed`, `map`, `flatMap`, `isCancelled`,
`isResolved`.~~ Listed in §18.1.4, unimplemented. They report honestly as
`E0413` now rather than leaking, so this is completing a documented surface.

**8. CLOSED 2026-09-26 (phase 1 merge, ERRATA E116).** ~~A task used in two combinators leaks `E0382`.~~ Awaiting consumes a task,
so `Task.any(a, b)` then `Task.allSettled(a, b)` is a use-after-move that rustc
reports. Needs a Jux diagnostic. Alongside it, an exception inside a `Result`
renders its Debug internals (`Err(Exception { __parent: ... })`) rather than its
message.

**9. CLOSED 2026-09-25 (`dc929a7`, ERRATA E107).** ~~The checker does not consult `@RustDefault`.~~
`juxc_tycheck::defaults::user_type_has_default` returns `true` for any external
class, which E97's markers make false, so `new Holder[3]` over a record that
lost `Default` is reported by rustc. About five lines, but it changes
`member_has_default` for every foreign type at once, so it wants the corpus as
its guard.

**10. CLOSED 2026-09-25 (`dc929a7`, ERRATA E107).** ~~Enums are not covered by the derive rule.~~ A Jux `enum` variant holding a
non-`Clone` foreign payload still goes through `decls/enums.rs`'s unconditional
derive. E97's fix applied at a third site; `ForeignDerives` and
`foreign_derives_of` already exist.

**11. CLOSED 2026-09-26 (phase 2 merge, ERRATA E115).** ~~Reading a field through a bound that names a GENERIC class~~
(`? extends Container<int>` then `it.someField`). Scoped into E100 rule 1
deliberately: the accessor bodies read through the shared handle at a fixed
depth, and a generic class's inherited field types are spelled in its parent's
type-parameter vocabulary, which the bound surface does not substitute through.
Calling a METHOD through such a bound does work.

**12. CLOSED 2026-09-26 (`5acd391`, ERRATA E108).** ~~Record components take no annotations.~~ Grammar §A.2.5 is
`record-component = annotation* type identifier`. A component becomes a
canonical-constructor parameter, so it is the natural next position after E104,
but it is its own AST node with its own construction sites.

**13. CLOSED 2026-09-26 (`5acd391`, ERRATA E108).** ~~Lambda parameter annotations cascade.~~ `(@Tag int a) -> a` produces five
errors rather than one clean diagnostic. The grammar is right to refuse them
(§A.2.9 `lambda-param = type? identifier`), so this is refusal quality. The fix
is in `parse_lambda`'s lookahead (`crates/juxc-parse/src/exprs.rs:1855`), which
does not confirm a lambda at all when the parens open with `@`.

**14. CLOSED 2026-09-26 as stale (`5acd391`): nested-type flattening already covered it; now pinned by `tests/ui/annotation_target_nested_type`.** ~~`check_annotation_applications` does not recurse into `nested_types`.~~
Pre-existing: a nested class's own `TYPE`/`METHOD`/`FIELD` annotations were
never checked either. Adding the recursion would newly fire `E0470`/`E0472`/
`E0473` across the corpus, so it needs its own change and its own corpus run.

### Build, tooling, diagnostics

**15. CLOSED 2026-09-26 (phase 1 merge, ERRATA E116).** ~~Linking and target selection leak raw rustc and cargo.~~ An unlinkable
`@extern(lib="c")` dumps the whole `link.exe` command line; an uninstalled
`--target` leaks `E0463` across seven crates; an unused
`[ffi.*] linkage="framework"` entry kills the build. The rustc-leak remapper
already exists for the compile path and is the model.

**16. CLOSED 2026-09-26 (`5acd391`, ERRATA E110).** ~~No lint configuration and no `-Werror` (§D.5.4).~~ W0820 is additionally
dropped by `jux check`, so "deny unjustified unsafe" cannot be expressed at all.

**17. CLOSED 2026-09-26 (phase 1 merge, ERRATA E116).** ~~`secondary_spans` and `code_action` have zero producers.~~ The schema and
both renderers support them; the only `.with_label` call sites in `crates/` are
LSP test fixtures. Wiring roughly ten diagnostics would deliver most of
§D.1.3's promised quality for little work.

**18. CLOSED 2026-09-26 (`5acd391`, ERRATA E109).** ~~Diagnostic polish.~~ `E0464`'s sibling message at
`crates/juxc-tycheck/src/check.rs:6052` still says "Drop `final`/`const`" rather
than echoing the written keyword, now that `FinalKw` exists to ask. `E0203`
covers only the four non-escapable Rust words, and `E0305`'s text claims "every
other reserved word is fine", which is false.

**19. CLOSED 2026-09-26 (`b5e44a1`).** ~~The IntelliJ plugin has no E0464 reassignment inspection at all~~, so the
`final` for-each binder's meaning is parsed and recorded but nothing in the
editor consumes it.

**20. CLOSED 2026-09-26 (`b5e44a1`).** ~~`JuxNamesValidator.isIdentifier` rejects any keyword~~, so the platform
Rename dialog still refuses renaming a package segment to `type` or `record`,
although ERRATA E78 makes the resulting path legal.

**21. CLOSED 2026-09-26 (`5acd391`).** `tools/java-differential/run.sh` did not honour a shared
`CARGO_TARGET_DIR`** when locating the built compiler; it needs `JUX_BIN`.

### Deferred by decision, not by oversight

**22. CLOSED 2026-09-26 (`b5e44a1`): grammar drift fixed, `jux.tmbundle/Syntaxes/`, an unpublished extension at `editors/vscode/`.** ~~A VS Code extension.~~ `juxc-lsp` answers 16 LSP methods correctly and no
VS Code user can reach any of it, which contradicts the "one server, many
editors" architecture. New surface rather than a fix. The TextMate grammar has
also drifted (`ref`, `typeof`, `weak` unhighlighted) and `jux.tmbundle/` has no
`Syntaxes/`, so the documented install path does not load.

**23. CLOSED 2026-09-26 (phase 8, ERRATA E121).** ~~The §CR representation selector is unbuilt.~~ The selector picks Inline, `Box`, `Rc`, `Rc<RefCell>`, `Arc` or `Arc<Mutex>` per class; what it leaves on a more general tier than §CR.3.3's table, and why, is in the ERRATA entry. Every class is
`Rc<RefCell>`, including the spec's own "zero heap allocation" example. This is
the single largest scoring hole (§CR at 44%), but it is a performance promise,
not correctness, and §CR.9 sequences it as a later phase. It also means
`E0950`-`E0952` are unreachable and should be marked so. (Phase 8, tier 1:
those numbers belong to the free-operator checks; §CR.7's diagnostics are now
`E0953`-`E0955`, ERRATA E121.)

**24. CLOSED 2026-09-26 (phase 4 merge, ERRATA E117).** ~~User-package-to-user-package bare-name capture.~~ A bare name in
`package a;` can still reach a type in an unrelated `package b;` through the
cross-package fallback. Left deliberately in E96: it is what makes `E0416` fire
at all for user code. §M.16's ladder is written so tightening it later is a
refinement, not a contradiction.

**25. CLOSED 2026-09-26 (phase 2 merge, ERRATA E113).** ~~`const_eval`'s constant lookup is still name-keyed~~ (`ConstCtx` carries
no package). Cannot bite today because `jux.std` declares no constants; if one
is ever added, `ConstCtx` needs a package.

### Found while closing gap 1

**26. CLOSED 2026-09-26 (phase 2 merge, ERRATA E112).** ~~A nullable INSIDE a collection prints Rust's form.~~ `Vec<int?>` holding
`3` and `null` prints `[Some(3), None]` rather than `[3, null]`. The
collection's own `Debug` writes its elements, and E107's run-time
normalization only sees the outer type's name. Not a regression: the output
was the same before E107. Recorded under E107's "Still open".

### Rust leaking through the abstraction (added 2026-09-26)

**27. CLOSED 2026-09-26 (phase 1 merge, ERRATA E116).** ~~rustc's borrow errors reach the user verbatim.~~ When the backend emits
invalid Rust, `cargo_build` / `build_emitted_crate` (`crates/juxc-driver/src/lib.rs`)
bail with rustc's text. `source_map.rs` remaps the `-->` arrow to the `.jux`
line, but the message stays `error[E0502]: cannot borrow ... as mutable`, it is
not a `Diagnostic` (so `--diagnostic-format json` never sees it), and the exit
code is 1, not the ICE's 101. Every such error is a compiler bug (ERRATA E23),
and should say so in Jux terms.

**28. CLOSED 2026-09-26 (phase 1 merge, ERRATA E116).** ~~A missed hoist panics with Rust's `already borrowed`.~~ Class handles are
`Rc<std::cell::RefCell<..>>`; when the backend misses one of the §CR.4.1 hoists
the program dies with `thread 'main' panicked at src\main.rs:L:C: already
borrowed: BorrowMutError`, naming an emitted-Rust line and no Jux type.

**29. CLOSED 2026-09-26 (phase 7, ERRATA E119).** ~~Nothing catches those conflict shapes before a user does.~~ Per E23 Jux
has no user-visible borrow checker and is not getting one, so the fix is a
compiler self-check over the emitted code, run in the test gate, not a user
diagnostic.

### Found by the leaker app (added 2026-09-26, see `leaker/LEAKS.md`)

**30. CLOSED 2026-09-26 (ERRATA E127).** ~~Borrowed foreign parameters and closures (L1, L8, L9, L10, L12, L14, L15).~~ A Jux lambda passed to an egui container receives a clone of `&mut Ui`; how a `Ui` parameter is passed depends on what the body calls; a field lent to a `&mut` parameter is a temporary copy; writes through `@RustRefOut` accessors are lost; hoisting moves `&mut` handles and Strings.

**31. CLOSED 2026-09-27 (ERRATA E128).** ~~Bindgen type mapping (L2-L7, L11, L13, L16-L21).~~ `impl Into<T>` and blanket-impl traits, trait-object parameters (non-existent emitted path), erased `Arc`s, associated constants, tuple structs, struct-like enum variants, `HashMap`/`BTreeMap` parameters, keyword-named fields corrupting a stub, collection and `Path` conversion, duplicate/mislabelled types, re-exported crates, stale stubs after removing a dependency. Fixed: generic slots (`@RustImpl` with `@RustFrom`/`@RustBlanket`/`@RustImplementedBy`), erased pointers (`@RustArc`, `@RustDerefOut`), associated constants, tuple structs, named-field variants, maps under their own names, crate families with one declaration per Rust type, declared-only stub loading, keyword-named fields, `E0907` for a stub that reads as statements, and read-only closure arguments (`@RustClosureShared`, `E0488`).

**32. CLOSED 2026-09-26 (gap 32 stream, ERRATA E122-e).** ~~Language and diagnostics found along the way (L22-L28).~~ Parse-error recovery inside a class (L22, fixed), `usize` counts (L23, kept a `uint` by the spec; the E0410 help names the length), a record's unqualified static call (L24, fixed, enums too), a user class shadowed by a foreign type in codegen (L25, fixed), a parameter named `r` lowered as a collection (L26, root cause: constructor locals leaked into the backend's base type scope; fixed and minimized), a terse E0900 without `--verbose` (L27, fixed), Rust naming showing through (L28: stubs no longer need nightly, hover pinned clean; verbatim names stay per §G.4).

**33. CLOSED 2026-09-27 (ERRATA E129).** ~~Make a Rust leak impossible to ship, with one guard at every exit.~~ One detector (`juxc_diagnostics::leak`) guards the diagnostic renderers, the binaries' error exit, the language server and the ICE report; the emitted program throws Jux exceptions for out-of-range positions, empty-array `pop()`, negative array sizes and failed `File` operations, and its panic hook reports what is left (`panic: ...` / `Exception in thread "main" ...` with the `.jux` line; a stack overflow on Windows too). `E0905` replaces the cargo passthrough; `E0436`, `E0900`, `E0904`, the self-check note, the ICE report and `--help` no longer name Rust tools or types. `bin/juxc/tests/leak_sweep.rs` holds every pinned output to the detector (allowlist empty), and `bin/jux/tests/runtime_failures.rs` runs five deliberately failing programs. Not guarded then, closed by gap 38: a stack overflow off Windows, foreign `Display`/`Debug` text beyond cells and nullables, a library's error under its Rust type name, a crate's doc comments on hover.

**34. CLOSED 2026-09-27 (ERRATA E130).** ~~An accepted program that meets a backend bug does not build.~~ The backend has a fast and a safe lowering level, per function (`lowering_level.rs`): the safe one copies every local read, binds every operand and argument that reads a cell or runs Jux code, gives the classes involved `rc-refcell`, writes a primitive or `String` `var`'s type out, and makes each value arm of a `String` switch an owned `String`, which is designed to eliminate `E0382`, `E0499`, `E0502`, `E0505`, `E0506`, `E0507`, `E0594`, `E0596`, `E0716`, `E0282`, `E0283` and `E0308` in match arms. When rustc refuses the fast crate, the driver (`self_heal.rs`) traces each error through the `// JUX:` markers to its Jux function, lowers those safe and rebuilds (twice at most), then the whole program once, and only then reports the fast build's `E0900`. A heal is cached in `.jux-safe-fns`, silent but for a `--verbose` note, and a failure under `JUX_SELFCHECK=1` naming the function and the original rustc error. `JUX_FORCE_SAFE=1` forces the safe level everywhere; `bin/jux/tests/safe_mode.rs` holds 97 examples to the corpus's own output under it (the whole corpus, 444 examples, also passes), and breaks a function on purpose (`JUX_TEST_BREAK_FAST`) to watch the heal. Of the two E0900s found today, the `switch` arm (`E0308`) is rescued; `$v` over a generic `T` (`E0277`) is not, and is gap 35's. Not covered: errors that are not about ownership or inference (`E0277`, `E0599`, a mismatched type outside a match arm).

**38. CLOSED 2026-09-27 (ERRATA E134).** ~~The four leaks gap 33 left known.~~ A stack overflow on Linux and macOS (x86_64, aarch64) is reported as on Windows, `panic: stack overflow: ...`, status 101, by a `SIGSEGV`/`SIGBUS` handler on an alternate signal stack that compares the fault address with the thread's recorded stack (main and `Worker` threads; run under Linux, type-checked for all four targets). A foreign value with no `Display` prints its `Debug` structure in a record's form (`FromUtf8Error(bytes: [104, 255], error: Utf8Error(valid_up_to: 1, error_len: 1))`), with cells and wrappers stripped, `Some(x)` as `x`, `None` and `<uninit>` as `null`, and nothing inside quotes rewritten. A library's `Err` is a Jux exception: `NumberFormatException` (new) for a number that does not parse, `FileNotFoundException`/`IOException`, `EncodingException`, `ArithmeticException`, `IllegalArgumentException`, and `LibraryException` (new, naming the library) for the rest; a clause naming the library's own type still gets its value. A crate stub's doc comment is shown on hover and in completion rewritten in Jux terms (links as Jux names, `T?`, no borrows or lifetimes, no Rust code blocks or `# Safety`), and held to the leak detector, which now knows rustdoc. Tests: `bin/jux/tests/runtime_failures.rs`, `examples/foreign_debug_text.jux`, `examples/foreign_errors_as_jux_exceptions.jux`, `examples/rust_error_uncaught.jux`, `examples/apps/json_lib`, the backend's `foreign_debug_text_is_laid_out_the_jux_way`, `crates/juxc-lsp/src/crate_doc.rs` and `hover_shows_a_crate_doc_in_jux_terms`.

**37. CLOSED 2026-09-27 (ERRATA E132, E133).** ~~Two types of one simple name in a crate family; deprecated crate items; aliases (LEAKS L29, L39).~~

- **L29: egui's `Frame` cannot be named from `rust.eframe`.** Fixed in
  general, not for `Frame`: a crate family's stub has a nested package for
  every module the host publishes a member's items under, following the Rust
  path (`rust.eframe.egui.Frame` is `eframe::egui::Frame`, beside
  `rust.eframe.Frame`). A type that loses a shared simple name is declared
  there; every other member type is aliased there; every signature naming a
  shared name writes the exact type. Importing both by their simple name is
  `E0303`; two wildcards bringing both are `E0303` at a use only, as in Java.
  The leaker's side navigation is dark again.
- **L39: deprecated crate methods are not marked.** A crate's
  `#[deprecated]` item carries `@Deprecated(message = "<note>")` in its stub,
  and a call to anything `@Deprecated` is the new warning `W0491`; rustc's own
  warning is silenced in the emitted crate.
- **Aliases (added to the gap 2026-09-27).** `import x.y.Z as W` works
  everywhere a plain import does (supertypes, upcasts, patterns, nested types,
  and every other position), and a `type` alias is expanded with its
  parameters substituted everywhere, `new` and static calls included; both
  are rewritten before checking, so nothing downstream sees an alias. An alias
  cycle is the new `E0498`, the wrong number of type arguments `E0443`.
- **L40 (found on the way).** A static read held its lock to the end of the
  statement, so a lambda of the same statement reading the static again
  hung; a read now copies the value out and releases the lock at once.

Tests: `crates/juxc-bindgen/tests/leaks_fixture.rs` (a two-crate fixture
family), `bin/juxc/tests/family_names.rs` (built and run against the real
fixture crates), `examples/multifile/importalias`,
`examples/type_alias_everywhere.jux`, `examples/static_read_in_chain.jux`,
UI `ambiguous_wildcard_import`,
`import_alias_clash`, `type_alias_cycle`, `type_alias_arity`.

**36. CLOSED 2026-09-27 (branch `leaker-idiomatic`, LEAKS L30-L38).** ~~The shapes the idiomatic rewrite hit.~~

- **L30:** a lambda argument bound to a `let` by argument hoisting lost its
  parameter type (E0282).
- **L31:** a `static final` of a non-class type computed by a call became a
  Rust `const` (E0015).
- **L32:** field initializers were never type-checked, so `!!` in one emitted
  nothing.
- **L33:** a field lent to a crate's static function or constructor
  (`TextEdit.singleline(name)`, `new DragValue(line.qty)`) was a copy or a
  shared borrow, the representation selector did not count it as a write, and
  hoisting ended the widget's borrow early.
- **L34:** a lent `Ui` was moved into an argument temporary.
- **L35:** a crate closure's `&str` argument stayed `&str`, and `c ? null : x`
  did not wrap `x` without an announced nullable target.
- **L36:** a trailing `return` lending a collection kept its guard past the
  local (E0597).
- **L37:** the borrow self-check took a crate method named like a Jux method
  for the Jux one.
- **L38:** `local.method(ui)` passed `ui` by value because the one-name
  receiver was read as a class name. A crate function's closure type (such as
  `run_ui_native`'s `(Ui, Frame)`) was also read only in the caller's imports,
  so it left the lambda's parameters untyped.

Tests: `bin/juxc/tests/leaker_idioms.rs` and
`examples/field_initializer_calls.jux`.

### Release-blocker sweep (added 2026-09-27)

**35. CLOSED 2026-09-27 (ERRATA E131).** ~~Re-measure the 2026-09-24 release-blocker list and fix what still fails.~~ Every item of FEATURES-TODO's "Release blockers measured 2026-09-24" was reproduced against a release build with real Jux syntax. Seventeen had been closed by gaps 1-33 and four only in part; the rest still failed. Everything that failed is fixed at source, each with a regression test (`examples/release_blockers.jux`, `examples/no_entry_point.jux`, seven `tests/ui` cases, `bin/jux/tests/release_blockers.rs`). Also from E129's "not guarded" list: the bound-crate toolchain wording, `juxc explain` (now user-facing, held by a unit test over every code) and the `jux:` lines.

| # | Blocker (2026-09-24) | Outcome | Fix / test |
|---|---|---|---|
| 1 | `ref int acc = total;` aliases nothing | already fixed | E90; `examples/ref_bindings.jux` |
| 2 | `observer<T>` parameter attaches nothing | already fixed | E91 |
| 3 | `$v` of a `var` double prints `2` | already fixed; the same form over a generic `T` still failed | E90-E93; gap 35 makes `$v` the `${v}` expression; `release_blockers.jux` |
| 4 | `task.cancel()` skips `finally` | already fixed | E92; `cancellation_and_timeouts.jux` |
| 5 | static-initializer cycle hangs | already fixed | E93 (`E0497`) |
| 6 | user type named `T`/`Vec`/`String`/... | already fixed | E96 |
| 7 | `spawn` of a class; `Task` surface; `Task` typo | fixed by E87/E94/E114, except `spawn(f())` of a non-async `f` (E0900) and `Task.yield()` | `E0708`, `Task.yield()`; `ui/spawn_not_a_task`, `release_blockers.jux` |
| 8 | foreign non-`Clone + Debug` type in an aggregate | already fixed | E97, E118, E120 |
| 9 | `sealed class` beyond one binding | already fixed | E101 |
| 10 | function type over a polymorphic class | already fixed | E98 |
| 11 | root `main.jux` subclass of a packaged class | still failing | `crate::` path in the base's `From` impls; `bin/jux/tests/release_blockers.rs` |
| 12 | `print(switch ...)` with a `String` and a literal arm | still failing | arms agree as a `? :`'s do; `release_blockers.jux` |
| 13 | `ref` class parameter breaks the build; no `W0490` | build fixed before gap 35; `W0490` still missing on a parameter | `W0490` on parameters; `ui/ref_param_on_reference_type` |
| 14 | keyword-named property (`loop`) | still failing, and the formatter printed Rust | escaped getter call; formatter output dropped; `release_blockers.jux` |
| 15 | property getter String intrinsics | still failing (also widening, promotion, `charLength`) | synthesized `this` gets its own span; `release_blockers.jux` |
| 16 | record implementing an interface calls `this.inner.eval()` | still failing | record methods take `&self`; `release_blockers.jux` |
| 17 | wildcards over a class bound | already fixed | E100, E115 |
| 18 | `Box<int?>`; `HashMap<K,int>` with `double` | already fixed | E107; `E0933` |
| 19 | annotations on parameters and locals | already fixed | E104 |
| 20 | `Result.ok(1)` / `Result.err(e)`; `unwrap` on `Err`; `Result.from` | `ok`/`err` still E0900; `unwrap` throwing `IllegalStateException` is not a bug (E56); `Result.from` not provided | `E0413` naming `Result.Ok`; `ui/result_static_constructor`; `Result.from` left open |
| 21 | `@entry` does nothing | already fixed | E105 |
| 22 | `[lib]` plus two `[[bin]]` | already fixed | E103 |
| 23 | `jux.toml` unvalidated | already fixed | E106 (`E0903`, `W0901`, `W0902`) |
| 24 | `@export(convention = "Stdcall")` ignored | still failing | `E0527`; `ui/export_convention_unsupported` |
| 25 | no lint levels, no `-Werror` | already fixed | E110 |
| 26 | `Integer.MAX_VALUE`, `Zork.thing` leak | still failing | `E0301` with the `int.MAX_VALUE` hint; `ui/unknown_qualified_name` |
| 27 | `new app.model.Secret()` skips `E0416` | still failing | FQN checked against its declaration; `bin/jux/tests/release_blockers.rs` |
| 28 | link and target selection leak | already fixed | E116 (`E0904`, `E0906`, `E0908`) |
| 29 | null narrowing in an interface `default` method | still failing | bare call typed by the interface's method; `release_blockers.jux` |
| 30 | `AsyncMutex<T>` as a class field | still failing | owned guard, `Debug`; `release_blockers.jux` |
| 31 | `assertThrows<E>` with a user exception | already fixed (no ERRATA names it) | the probe needs `throws` on the callee (E0711) |
| 32 | empty file; interface cycle; `new int[-1]`; printing a curried lambda | all still failing | `E0327`, `E0434` for interfaces, run-time size, `<fn>`; `no_entry_point.jux`, `ui/interface_inheritance_cycle`, `release_blockers.jux` |
| 33 | keyword as a parameter or local name cascades | still failing (E109 fixed only `E0305`'s text) | `E0204`; `ui/keyword_binding_name` |

Also found and fixed on the way: `W0457` said classes are "`Rc`-refcounted" (eleven pinned outputs updated by hand).

Left open: `Result.from(() -> ...)` (§X.5.4) is not provided; it is a clean `E0413` with help. A stack overflow off Windows and a foreign `Display` stay as E129 left them.

### Generics sweep (added 2026-09-28)

**39. CLOSED 2026-09-28 (ERRATA E135).** ~~Deeply nested generics and intersection bounds must be no issue.~~ Two bugs were reported against a probe (`Registry<T extends Named & Aged, K, V extends Vec<Pair<K, T>>>` was E0900 "cannot find trait `VecKind`"; `t.ident() + t.age() + t.score()` through `<T extends Base & Named & Aged & Scored>` was E0900 "cannot add `f64` to `isize`"), then the area was swept with probe programs, each run with `JUX_SELFCHECK=1`. Every failure is fixed at source or is now a Jux diagnostic. A member reached through a bound is typed by the bound (numeric promotion follows); a bound on a type nothing else extends (Rust type, record, enum, primitive, `String`) is `E0459` (new), two classes in an intersection `E0419` (new), a wrong type-argument count at any depth `E0443`, extending a Rust type `E0420`; `<R extends K>` converts an `R` into a `K` (`R: Into<K>`, written in when the body needs it); a method's `<T>` shadows the class's; a constructor may call `super(...)` and do other work before assigning a field of a type parameter; an inherited body and an inherited call are read through the `extends` chain's parameter map. Tests: `examples/intersection_bounds.jux`, `examples/generic_supertypes.jux`, `examples/dependent_bounds.jux`, `examples/generics_deep_nesting.jux`, seven `tests/ui` cases.

| Probe | Before | Now | Fix / test |
|---|---|---|---|
| `Registry<T extends Named & Aged, K, V extends Vec<Pair<K, T>>>` | E0900 (`VecKind`) | `E0459`: a Rust type admits only itself; write `Vec<Pair<K, T>>` or bound by an interface | `check_declared_bounds`; `ui/bound_on_foreign_type` |
| bound on a record, an enum, `int`, `String` | E0900 | `E0459` | `ui/bound_on_final_value_types` |
| bound on a `final` class | worked | works | `intersection_bounds.jux` |
| `class MyVec extends Vec<int>` | E0429 listing every `Vec` method | `E0420` | `ui/extends_foreign_type` |
| `t.ident() + t.age() + t.score()`, `t.id + t.score()`, `t.score() * t.big`, `<`, `?:`, `double` slot, `+=`, `/ 4.0`, `* f` through bounds | E0900 (`f64` + `isize`) | promotes | `param_bound_tys` in `infer.rs`; `intersection_bounds.jux` |
| `T extends Base & I1 & I2 & I3` on a class, interface (with default), record, method; members of every bound; `T` passed as `Base`/`I1`/`I2`, returned, stored in fields, `Vec`, `HashMap` | worked (once typed) | works | `intersection_bounds.jux` |
| `T extends Box<int> & Named & Tagged<String>` (generic class and interface bounds) | worked | works | `intersection_bounds.jux` |
| class after interface in `&` (`T extends Named & Base & Aged`) | worked | works (§T.4.6 rule 1: order is not significant) | `intersection_bounds.jux`, `ui/bound_names_two_classes` |
| two classes in `&` | accepted silently | `E0419` (names the class to keep when related) | `ui/bound_names_two_classes` |
| argument missing one bound of an intersection | `E0446` | `E0446`, per bound missed | `ui/intersection_bound_one_missing` |
| wrong type-argument count deep in a nesting (local, field, parameter, bound) | E0900 (rustc E0107) | `E0443` at the inner type | `check_type_arity`; `ui/wrong_type_argument_count_nested` |
| `class A<T, U> extends B<Pair<T, U>> implements I<Vec<T>>, J<U>` with `super(..); ts.push(t); this.u = u;` | E0900 (`U::default()`) | works | prefix lift past `super` and blind statements; `generic_supertypes.jux` |
| generic interface hierarchy `I2<T> extends I1<Pair<T,T>>`, `I3<T,S> extends I2<T>`; interface for concrete args; default methods | worked | works | `generic_supertypes.jux` |
| `IntBox extends Box<int>` inheriting `Box<T> with(T nv) { return new Box<T>(nv); }` | E0900 (`T` not found) | works | inherited body emitted through the parameter map; `generic_supertypes.jux` |
| `Squad<Person>.add(new Student(..))`, `add(T)` inherited from `Roster<T>` | E0900 (E0308) | works | argument type composed along `extends`; `generic_supertypes.jux` |
| overriding a generic method on an extended class | `E0438` advising `final`/`sealed` (neither works) | `E0438` advising non-generic, `private` or `static`; generic base and package-private method covered (were E0900) | `ui/generic_method_on_generic_base` |
| F-bounded `E extends Entity<E>`, `T extends Ord<T>` (a user `Ord`), `where T has operator<=>` | E0900 with a user `interface Ord` (`impl Ord for`) | works | `std::cmp::Ord` by path; `dependent_bounds.jux` |
| `<K, V extends Holder<K>>` on a class and a method | worked | works | `dependent_bounds.jux` |
| `<T> T echo(T t)` in `class Shelf<T>` | E0410, then E0900 (E0403) | works | shadowing in the checker, rename in the emitter; `dependent_bounds.jux` |
| `<U extends T> void putSub(U u) { items.push(u); }`, `<R extends A>` storing `R`, `class Chain<K, V extends K>`, `<K, V extends K>` free function, `<V extends K>` interface default | E0900 (E0308 / `V: K`) | works | `ParamInto` coercion + `Into<K>`; `dependent_bounds.jux` |
| static generic `Pair.of("a", 1)` then `var` | E0900 (`&str` + float) | works | static call read through its bindings; `dependent_bounds.jux` |
| interface default body `items().push(k)` | E0900 (no `push` on the handle) | works | bare call typed as `this.items()`; `dependent_bounds.jux` |
| 4 and 6 parameters, 5 and 6 levels; `HashMap<.., Vec<Pair<.., HashMap<.., Vec<Tri<..>>>>>>` | worked | works | `generics_deep_nesting.jux` |
| function types / lambdas over nested generics, generic record, payload enum, async of a nested generic, `var` through nested calls, diamond | worked | works | `generics_deep_nesting.jux` |
| `? extends Pair<String, Vec<int>>`, `? extends Person`, `? super Student` | worked | works | `generics_deep_nesting.jux` |
| `type Reg<T> = Registry<T, String, Vec<Pair<String, T>>>`, `Table<V>`, `Grid` | worked | works | `generics_deep_nesting.jux` |
| `Outer<T>.Inner<U>` (static nesting, §M.9) | worked | works | `generics_deep_nesting.jux` |
| one parameter down `Roster<T> <- Squad<T> <- StudentSquad` under `Base & Named & Aged` | E0900 on `Squad<Person>.add` | works | as above; `generics_deep_nesting.jux` |
| `Task<Pair<..>> t = spawn(..)` (a written `Task<T>`) | E0900 | works (gap 39b) | one task type for `spawn` and `Worker.spawn`; `examples/task_type_written.jux` |
| visitor `<R> R accept(Visitor<R> v)` on an interface / an extended class | E0435 / E0438 | works (gap 39b) | dispatch on the object's type; `examples/generic_virtual_methods.jux` |
| `class Chain<K, V extends K>` extended, storing a `V` as a `K` through the base | E0900 | works (gap 39b) | `V: Into<K>` on the dispatch trait; `examples/bounded_params_in_hierarchy.jux` |

~~Left open: the written `Task<T>` above; a generic method that can be overridden on an extended class (visitor-style `<R> R accept(Visitor<R>)`) stays `E0438`, as §D.4 has always said, because its dispatch goes through a trait object.~~ All three closed by gap 39b (ERRATA E136).

**39b. CLOSED 2026-09-28 (ERRATA E136).** ~~The three items gap 39 left open.~~ A written `Task<T>` is the handle `spawn` returns, and `Worker.spawn` returns the same handle (a worker's failure is now rethrown where it is awaited); a task is a copyable handle, so it goes in fields, parameters, returns, collections and `Task.all`. A method with type parameters of its own reached through an interface or an extended class (the visitor pattern) is dispatched on the object's own type: the trait keeps the method `where Self: Sized`, and the handle's impl asks the object what it is (`JuxDynAny`) and calls that type's method, which the Rust compiler monomorphizes for the call's own type arguments, so no set of type arguments has to be closed, only the set of types, which a whole-program compile always has. The two shapes that cannot be found that way (a subtype with a type parameter its supertype does not fix, an anonymous class) are `E0438` with the reason. `class C<K, V extends K>` states `V: Into<K>` on its dispatch trait. Tests: `examples/generic_virtual_methods.jux`, `examples/task_type_written.jux`, `examples/bounded_params_in_hierarchy.jux`, `tests/ui/generic_method_dispatch_limits`.

| Probe | Before | Now |
|---|---|---|
| `Task<int> t = spawn(f())`, `Task<Pair<String, Vec<int>>>`, a `Task<int>` field, parameter, return, `Vec<Task<int>>`, `Task<Vec<int>> all = Task.all(..)` | E0900 | works |
| `Task<int> w = Worker.spawn(..)`; `await`, `blockingGet()`, `join()`; a worker that throws | E0900 / "worker task aborted" | works; the exception is caught where awaited |
| visitor over an interface: `Visitor<int>`, `Visitor<String>`, `Visitor<Expr>` | E0435 | works |
| visitor over an abstract generic base `Tree<T>`: `Count`, `Render`, `Mirror` (`R = Tree<T>`), 3 subclasses | E0438 | works |
| a subclass fixing the base's argument (`Zero extends Tree<int>`, `Ints implements Bag<int>`) | E0438 / E0900 | works |
| a generic default method one implementer overrides, called through the interface | E0435 | works (override and default each where they belong) |
| `R` the caller's own type parameter (`<R> Vec<R> visitAll(.., Visitor<R> v)`, `<T, R> R foldAll(Vec<Bag<T>> ..)`) | E0435 | works |
| a generic method inherited and never overridden (`Tree.fold`) | E0438 | works |
| `class Weird<T, U> extends Tree<T>` with a generic virtual method | E0900 (after E0438 was lifted) | `E0438` naming `U` |
| an anonymous class of an interface with a generic method | E0435 | `E0438`: name the class |
| `LoudChain<K, V extends K> extends Chain<K, V>`, `PetChain extends Chain<Pet, Cat>`, through `Chain<..>` | E0900 | works |

~~Found on the way and left open (predates gap 39b): `class PetStore implements Store<Pet>` overriding `<V extends K> int addAll(Vec<V>)` as `<V extends Pet>` is E0900, because the interface's `V: Into<K>` and the class's `V: PetKind` are different Rust bounds (ERRATA E136, known boundary).~~ Closed by gap 39c.

**39c. CLOSED 2026-09-28 (ERRATA E137).** ~~What gap 39b still refused.~~ An override that bounds its type parameter by the type its supertype fixes (`<V extends Pet>` for `<V extends K>`) lowers the bound as the trait does, `V: Into<Pet>`, and converts a `V` where one of its members is used; the dispatch from the supertype calls a `__jux_via_` twin generic over `K`. Every anonymous class of a Jux type is lifted to a named class (`__JuxAnon_<Target>_<source>_<n>`, printed `Target$anon`) and the program checked again, so an anonymous visitor, an anonymous implementer of an interface with a generic method and an anonymous subclass of an extended class are all ordinary classes. A subtype with a type parameter its supertype does not fix is dispatched once per instantiation the program builds, closed as a fixpoint over `new` sites and generic calls with their contexts. `E0438` is left for polymorphic recursion that builds ever-larger types, the one case with no finite set. Tests: `examples/bounded_method_overrides.jux`, `examples/anonymous_classes_dispatch.jux`, `examples/unfixed_subtype_params.jux`, `tests/ui/generic_method_dispatch_limits`.

| Probe | Before | Now |
|---|---|---|
| `interface Store<K> { <V extends K> int addAll(Vec<V>); <V extends K> String describe(V v); }`, `PetStore implements Store<Pet>` overriding with `<V extends Pet>`, through `Store<Pet>` and directly; `AnyStore<K>` beside it | E0900 | works |
| the same over `abstract class Shelf<K>` with `PetShelf extends Shelf<Pet>` and `NamedShelf extends Shelf<Named>` (an interface bound) | E0900 | works |
| anonymous `Visitor<int>` / `Visitor<String>` passing `this` on as the visitor | E0900 | works |
| anonymous implementer of `interface Expr { <R> R accept(Visitor<R>) }` capturing two locals | E0438 | works |
| anonymous subclass of an extended abstract class (generic method, captured local), in a `Vec<Shape>` with a named subclass | E0438 / E0900 | works |
| anonymous class in generic code using the caller's `T` (`<T> Box<T> boxOf(T value)`) | worked (inline) | works (lifted), prints `Box$anon@..` |
| `Weird<T, U> extends Tree<T>` built with `U` written out, bound by `<A, B> wrap(A, B)`, and `Vec<X>` in `Maker<X>.make` | E0438 | works |
| `wrap("x", 7)`: the literal where `B` is the parameter's type | built a `Weird<String, i32>` | builds `Weird<String, int>` |
| `grow<A, Vec<B>>` inside `<A, B> grow(..)`, building a `Weird<A, B>` | E0438 (the old rule) | `E0438` naming `grow`: polymorphic recursion has no finite set |

**39d. CLOSED 2026-09-28 (ERRATA E138).** ~~An anonymous class could not reach the object it was built in (E137's known boundary).~~ The checker finds the enclosing object's members an anonymous class names bare (its own parameters and locals, captured locals and the members of the type it implements or extends win); the lifted class holds the enclosing object in `__jux_outer`, passed `this` at the construction (or the enclosing anonymous class's own `__jux_outer` when nested), and each such name is rewritten to go through it, so writes land on the enclosing object. Calls through the handle, and calls on an element of a collection field, release every guard first, so the enclosing object may be calling into the anonymous class while it writes. A constructor that hands `this` out runs against the finished object; `this` handed out as a class with subclasses is wrapped as its dispatch value. A private member of an enclosing class with subclasses is `E0437` at the anonymous class (the handle is the dispatch value, where it has no slot; reachable since gap 39e). `Outer.this` is not in the spec and is not added. Tests: `examples/anonymous_outer_access.jux`, `tests/ui/anonymous_outer_private_poly`.

| Probe | Before | Now |
|---|---|---|
| `count++; total += x; note("on" + x);` in an anonymous `Listener` built in a `Counter` method (`note` private) | does not build | works, seen on the `Counter` |
| the listener run by `fire`, which is iterating the `Counter`'s own `listeners` | does not build | works under `JUX_SELFCHECK=1` |
| anonymous visitor writing `sum`/`visits` while `walk` calls `items[0].accept(v)` | does not build | works |
| anonymous class inside an anonymous class, both reaching the `Counter` | does not build | works |
| anonymous class built in a lambda in a method | does not build | works |
| a parameter / a captured local named like a field; the superclass's own field named like one | worked (the enclosing object was out of reach) | works; the anonymous class's own name wins |
| anonymous class built in a constructor, stored in an interface-typed field (`onClick = new Listener() { .. }`) | does not build | works |
| `r.add(this)` in a constructor | E0900 | works |
| anonymous class in `Bag<T>` reaching `items` | does not build | works |
| anonymous class in an abstract `Widget`'s method reaching `hits`, `bump()` and the abstract `name()`, run on a `Knob` | does not build | works |
| `this` passed as `Base` from a method of an abstract `Base` with subclasses | E0900 | works |
| `b.hits += x` on a `Base`-typed value, `hits` package-private | E0900 | works |
| the same reaching a *private* member of that `Widget` | does not build | `E0437` at the anonymous class |
| a field initializer that uses the object (`int y = twice(3);`) | E0900 | E0900 (predates this; known boundary; works since gap 39e) |

**39e. CLOSED 2026-09-28 (ERRATA E139).** ~~Field initializers that use the object, and private members of a class with subclasses reached from an anonymous class.~~ An instance field initializer that uses the object runs against the finished object, as do all after it (and all of a class whose ancestor's do), the hierarchy's root first, before any `init` block or constructor body, once per object; `this` is in scope in it. A field with no value to hold meanwhile (a class, a several-method interface, a type parameter) gets a nullable slot while compiling, each read asserting it is set; `E0499` is a direct read of a field that holds no value yet and has no default. `E0981` is left for observers. A private member reached through a class's dispatch value, where visible, is renamed to a hidden non-private stand-in (`__jux_priv_<Class>_<member>`) declared only for compilation; `E0437` is retired. Tests: `examples/field_initializers_use_object.jux`, `examples/anonymous_private_members.jux`, `tests/ui/field_read_before_initialized`.

| Probe | Before | Now |
|---|---|---|
| `int y = twice(3);`, `int b = a + 1;`, `int c = this.b * 10;` | E0301 / E0900 | works |
| `Worker w = new Worker(this);`, then `int seen = w.report();` | E0900 | works |
| `Shape s = new Shape() { .. n * n .. };` (two methods), `Listener l = new Listener() { .. n += x .. };` | E0900 | works, writes seen on the object |
| `() -> int peek = () -> n + doubled;` | `E0981` | works |
| `Vec<Owner> registry = listOf(this);` | E0900 | works |
| `int early = this.late + 1; int late = 5;` | E0900 | `early` is 1 (reads `late` at 0) |
| `int boost = engine.power * 2; Engine engine = ..;` | E0900 | `E0499` |
| `Base`/`Kid` hierarchy with logging initializers, `init` blocks and bodies | E0900 | base.a; base.b; kid.c; base.init; base.body; kid.init; kid.body |
| `Kid(int x) { this(); .. }` | E0900 | initializers run once |
| `Stream<T>` with no constructor, `int size = count();` and a lambda over it | E0900 | works |
| abstract `Figure` whose initializer calls the subclass's override | E0900 | works |
| `this.w = new Worker(this);` in a constructor, `Worker w;` | E0900 | works |
| `hits += twice(x); hits++; note(..)` in an anonymous class in an abstract `Panel` (all private) | `E0437` | works |
| `other.hits`, `other.hits = 0`, `other.note(..)` in `Panel.absorb(Panel other)` | `E0437` / E0900 | works |
| `a.name` on a private field through `Animal` from `main` | `E0437` | `E0414` |

**39f. CLOSED 2026-09-28 (ERRATA E140).** ~~Overloaded private methods had no stand-in; construction ran every initializer of the hierarchy first (E21).~~ Construction is Java's: each class, root first, runs `super(..)`, its initializers and `init` blocks in source order, then its body; `this(..)` runs the initializers once. A base constructor calling an override reads the subclass's fields at their defaults; a field with no default value throws `IllegalStateException` when read before it is set (since E141, `NullPointerException`, as in Java). Each private overload has its own stand-in under one hidden name, so a call through a dispatch value picks the overload by its arguments. Tests: `examples/construction_order_java.jux` (checked against Java: `tools/java-differential/cases/83_construction_order_java`), `examples/private_overloads_dispatch.jux`; `44_init_order` no longer diverges.

| Probe | Before | Now |
|---|---|---|
| `Base()` prints, `Derived` field initializer prints (`44_init_order`) | base-field, derived-field, base-ctor, derived-ctor (declared divergence) | base-field, base-ctor, derived-field, derived-ctor (= Java) |
| base constructor calls an override reading `int v = 7` (`ctor_calls_method`) | 7 | 0 (= Java) |
| base constructor calls an override reading `String own = ..` (`construction_and_identity`) | `child hook sees child field` | `child hook sees` (the default `""`; Java prints `null`) |
| three-level hierarchy with `init` blocks between initializers, `this(..)`, override reading a subclass `Tag` field (`construction_order_java`) | E21 order | Java's order, the `Tag` read throws `IllegalStateException` (Java: `NullPointerException`) |
| `add(int)`, `add(String)`, `add(int, String)` private, from an anonymous class and through `other.` | E0900 | works |
| `scale(int)` / `scale(double)`, `show(int)` public beside `show(String)` private | E0900 | works |
| `add(x, name())` on the overloaded stand-in through a bound receiver | E0900 (rustc: the one-argument overload called) | calls `add(int, String)` |

**39g. CLOSED 2026-09-28 (ERRATA E141).** ~~Polymorphic recursion (`E0438`, or rustc's recursion limit); an early read threw `IllegalStateException`.~~ The functions, methods and classes on a polymorphic-recursion cycle (found by the E137 instantiation closure, closed over the class hierarchy) are instantiated at one erased type, `JuxErased`, a boxed value carrying its own type, text, `==` and hash; values are boxed into the family's type-parameter slots and unboxed out of them to the type the checker gave them, and the erased subtype's dispatch has one branch for every depth. Code off the cycle is compiled as before. `E0438` is left for a cycle with a bounded type parameter. An early read of a field with no default now throws `NullPointerException`; a `String` field still reads `""` (non-null by type). Tests: `examples/polymorphic_recursion_grow.jux`, `examples/polymorphic_recursion_nested.jux` (both checked against Java: differential cases 84 and 85), `examples/early_read_no_default.jux`, `tests/ui/generic_method_dispatch_limits` (the bounded case).

| Probe | Before | Now |
|---|---|---|
| `grow<A, B>` calling `grow<A, Vec<B>>`, building `Weird<A, B>` dispatched through `Tree<A>` | `E0438` | works, prints each depth, `accept` finds the `Weird` |
| `build<T>` calling `build<Pair<T>>` for `Nested<T> = Flat(T) \| Nest(Nested<Pair<T>>)` | E0900 (rustc recursion limit) | works: depth, size, printing |
| `sameAfter<T>(a, b, n)` comparing `Pair<..>` values `n` levels deep with `operator ==` | E0900 | works (`true` / `false`, as Java's `equals`) |
| `HashSet<Pair<int>>` of an erased class with `operator hash`, beside that recursion | E0900 (the program did not build) | works |
| a generic method `walk<T>` calling `walk<Pair<T>>`; `echo<T>` returning `T` through `echo<Pair<T>>` | E0900 | works |
| a `T?` field of an erased class read and written from outside | E0900 | works |
| `nest<T extends Named>` calling `nest<Wrap<T>>` | E0900 | `E0438` naming the bound |
| base constructor's override reads an unset `Gear` field | `IllegalStateException` | `NullPointerException` |

**39h. CLOSED 2026-09-28 (ERRATA E142).** ~~A bounded type parameter on a polymorphic-recursion cycle was `E0438`.~~ The erased value keeps each bound (and each supertrait of one) as its dispatch object, built where it is boxed, and the erased type implements each bound by forwarding to it; operator bounds are kept the same way as `==` and hash. A generic class's arithmetic operators now implement the Rust operator traits. `E0438` is left for a bound that names a type parameter (`T extends Ranked<T>`) and for a `const` parameter on the cycle. Tests: `examples/polymorphic_recursion_bounded.jux` (checked against Java: differential case 86), `examples/polymorphic_recursion_operators.jux`, `tests/ui/generic_method_dispatch_limits`.

| Probe | Before | Now |
|---|---|---|
| `nest<T extends Sized2>` calling `nest<Wrap<T>>`, `x.name()`, `x.size()`, `describe(x)` into `Named` at each of 6 depths | `E0438` | works, matches Java |
| `corral<A extends Animal>` calling `corral<Pen<A>>`, `a.sound()`, `a.legs()` (overridden) | `E0438` | works, matches Java |
| `pair<T extends Named & Aged>` calling `pair<Both<T>>` | `E0438` | works, matches Java |
| `compare<T>` / `sum<T>` with `where T has operator<=>` / `operator+`, recursing at `Box<T>` | E0900 | works |
| `T extends Sized2` passed where `Named` is expected (`Sized2 extends Named`), no recursion | E0900 | works |
| `climb<T extends Ranked<T>>` calling `climb<Up<T>>` | `E0438` | `E0438` (the bound names `T`) |

**39i. CLOSED 2026-09-28 (ERRATA E143).** ~~A bound naming the parameter (`T extends Ranked<T>`) or a `const` parameter on a polymorphic-recursion cycle was `E0438`.~~ Such a bound is reached through a generated adapter that unboxes erased arguments to the value's own type and reboxes results with the value's own table (interfaces from their declaration, classes from their dispatch trait); `const` parameters are kept as they are (they never grow). `E0438` is left for one case: a bound holding the parameter inside a Rust type (`T extends Rel<Vec<T>>`), whose only conversion is a copy that would break the list's sharing. Tests: `examples/polymorphic_recursion_fbounded.jux` (checked against Java: differential case 87), `examples/polymorphic_recursion_const.jux`, `tests/ui/generic_method_dispatch_limits`.

| Probe | Before | Now |
|---|---|---|
| `best<T extends Ord2<T>>` calling `best<Box<T>>`, `a.pick(b)` then `p.cmp(a)` on the result | `E0438` | works, matches Java |
| `deep<T extends Tagged<T, String>>` calling `deep<Wrap<T>>` | `E0438` | works, matches Java |
| `climb<T extends Shape<T>>` over an abstract class, calling `climb<Framed<T>>` | `E0438` | works, matches Java |
| `weave<T extends Joins<Pair<T>>>` calling `weave<Shell<T>>` | `E0438` | works, matches Java |
| `Chunk<T, int N>` built as `Chunk<T, 3>` on a cycle, `int[N]` field | `E0438` | works |
| `pack<T extends Rel<Vec<T>>>` calling `pack<Crate<T>>` | `E0438` | `E0438` (a copy would break the list's sharing); works since gap 40 |

### Collections by reference (added 2026-09-28)

**40. CLOSED 2026-09-28 (ERRATA E144).** ~~A collection was not the same collection on every channel.~~ Wave 1 (TODO.md 1b, 1c) made a collection the shared handle a class instance is; a probe of every channel a collection travels through found the ones that still moved or copied it, and each is fixed: a nullable local, parameter or field, a reassignment, a `switch` arm and a lambda's returned capture share the handle (they stopped at an internal compiler error, rustc E0382/E0507, and the nullable local did the same to a class instance); `new Vec<int>[n]` gives each slot its own collection. Across erasure the erased `Vec<JuxErased>` is LINKED to the typed `Vec<X>`, one storage seen from both sides, which retires `E0438` for `T extends Rel<Vec<T>>` and makes a collection of an erased parameter crossing into or out of the erased part of a program (an argument, a return, a field, a constructor argument) compile at all. `E0438` keeps one case, a foreign non-collection type holding the parameter (`Rel<Box<T>>`), which has no conversion of any kind. A write to a collection a `Worker.spawn` closure captured, which reached only the worker's copy, is `E0702`. Tests: `examples/collection_alias_channels.jux` (Java differential case 150), `examples/array_of_collections.jux`, `examples/polymorphic_recursion_collection_bound.jux` (case 151), `examples/polymorphic_recursion_collection_slots.jux` (case 152), `tests/ui/worker_collection_write`, `tests/ui/generic_method_dispatch_limits`.

| Probe | Before | Now |
|---|---|---|
| `Vec<int>? nv = v; nv!!.push(1); print(v.len())` (and `Node? n = a;`) | E0900 (rustc E0382) | works, matches Java |
| `addTo(v)` into a `Vec<int>?` parameter, then `v.len()` | E0900 | works, matches Java |
| `w = v; w.push(7)` then `v.len()` | E0900 | works, matches Java |
| `var p = switch (k) { case A -> v; ... }` then `v.len()` | E0900 | works, matches Java |
| `() -> Vec<int> s = () -> v; s().push(1)` | E0900 (rustc E0507) | works, matches Java |
| `h.maybe!!.push(5)` on a `Vec<int>?` field | E0900 (rustc E0507) | works, matches Java |
| `new Vec<int>[3]`, push into slot 0 | E0900 (rustc could not parse the default) | each slot its own list |
| `pack<T extends Rel<Vec<T>>>` calling `pack<Crate<T>>`, the value pushing into the caller's list | `E0438` | works, matches Java |
| a bound's list kept by the value and written to later; a list it hands back; a `HashMap<String, T>` | `E0438` | works, matches Java |
| `grow<int>(1, acc, 3)` into a `Vec<T>` parameter of an erased function | E0900 | works, `acc` sees the push |
| a `Vec<T>` field of an erased class, read and written from outside; a `Vec<T>` return | E0900 | works, matches Java |
| a `Vec<Vec<T>>` and a `T[]` handed to a bound on a cycle | `E0438` | works, matches Java |
| `pack<T extends Rel<Box<T>>>` | `E0438` | `E0438` (no conversion into `Box` exists) |
| `Worker.spawn(() -> { xs.push(1); ... })` on a captured `Vec` | the push reached only the worker's copy | `E0702` |

**40b. CLOSED 2026-09-28 (ERRATA E145).** ~~What gap 40 left open.~~ Every container a polymorphic-recursion bound can hold the parameter in converts between the erased code and the value's own: a collection or array is linked, a tuple, a `Box` and a nullable are rebuilt, an `Rc` or `Arc` crosses as the same pointer. `E0438` is retired; a foreign generic type with no known shape (`OnceLock<T>`) is the new `E0620`. A collection crossing into or out of a worker-shared class converts instead of failing to compile, and a WRITE to a copy read out of one is `E0702` (real `Arc<Mutex>` sharing was not taken: two reads of one collection in a statement would deadlock the non-reentrant lock, and the representation selector has no collection tier). A `var` lambda has the function type its parameters and body give it, and one with an untyped parameter that nothing uses is `E0453`. `int[]? xs;` parses as a local. An erased or generic container key orders and hashes. A linked collection copies only when the side accessing it changes, so a loop over one is linear. Tests: `examples/polymorphic_recursion_foreign_bounds.jux` (Java differential case 156), `examples/polymorphic_recursion_arc_bound.jux`, `examples/worker_shared_collections.jux` (case 157), `examples/var_lambda_types.jux` (case 154), `examples/nullable_array_slots.jux` (case 153), `examples/polymorphic_recursion_ordered_keys.jux` (case 155), `examples/stress_linked_collection.jux`, `tests/ui/erased_through_foreign_type`, `tests/ui/worker_shared_collection_copy`, `tests/ui/lambda_param_uninferable`.

| Probe | Before | Now |
|---|---|---|
| `T extends Pairs<(T, int)> & Maybe<T?> & Shares<Rc<T>> & Boxes<Box<T>>` on a cycle | `E0438` / E0900 | works, matches Java |
| the same with `Arc<T>` | `E0438` | works, `Arc.ptr_eq` holds |
| `T extends Rel<OnceLock<T>>` on a cycle | `E0438` | `E0620` |
| `var l = reg.getLog(); print(l.len());` on a worker-shared `reg` | E0900 | a copy, read |
| `var l = reg.getLog(); l.push(x);` | E0900 | `E0702` |
| `reg.items.push(x)`, `reg.items.len()`, `reg.items = xs`, `reg.addAll(xs)` | E0900 | work |
| `var gv = () -> v; gv().push(1);` | E0900 | works, matches Java |
| `var f = (x) -> x + 1;` never used | E0900 (rustc E0282) | `E0453` |
| `int[]? xs;` as a local | parse error | works, matches Java |
| `BTreeMap<T, int>` keyed by an erased or generic `T` | E0900 (rustc E0277) | works, matches Java |
| 10,000 pushes and reads through an erased function | quadratic | about 60 ms (debug build) |

### Crate-boundary sweep B (added 2026-09-28)

**Sweep B. CLOSED 2026-09-28 (ERRATA E1XX-SWEEPB).** ~~The leftovers of the crate-boundary work (E128, E132, E134).~~ A library's error is the closest Jux exception by what it is, not by a table: an `io::Error` by its kind (`AccessDeniedException`, `FileAlreadyExistsException`, `SocketTimeoutException`, `InterruptedIOException`, `EOFException`, `FormatException`, all new `IOException`s, Java's names; `IllegalArgumentException`, `UnsupportedOperationException`), Rust's own errors by type, a crate's error by the shape bindgen reads off its rustdoc (a timeout, a serde-shaped or format error, a `FromStr` error of a number or of anything else, a not-found name, one built from an `io::Error`, and per enum variant), then by the errors it wraps; `LibraryException` only when nothing is closer. A library's error value prints as that exception (`NumberFormatException: invalid digit found in string`) wherever it is shown: printed, interpolated, joined, inside a `Result` (which now prints for an error with no `Clone` too) and in generic code. Two items of one simple name inside one crate (naga's `Error`s) are each declared in the nested package of their module (`rust.naga.front.wgsl.Error`), every signature names the exact one, and two imports of them are `E0303`. The names inside bindgen's string markers and `implements` clauses are qualified the way signatures are, and their readers resolve qualified names. A stack overflow on a crate's own thread running a Jux callback is Jux's report on Linux and macOS (the callback records its thread; an unrecorded thread is judged by its stack pointer at the fault), run on Linux (WSL, static musl) and Windows, type-checked for macOS and aarch64. Found and fixed on the way: a fully-qualified crate function call or foreign enum variant (`crate::rust::...`, E0900), a qualified class slot read as the trait of its last segment, a borrowed static-call argument cloned, a foreign free function looked up by last segment, a `catch` of a crate's error enum that never matched. `rust.std` stays flat (§G.6.2.1). Tests: `crates/juxc-bindgen/tests/sweepb_fixture.rs` (real rustdoc JSON of a fixture crate in naga's shape), `bin/juxc/tests/crate_errors.rs` (built and run against that crate), `foreign_errors_are_classified_by_their_shape`, `examples/foreign_errors_by_shape.jux`, `examples/runtime_library_error_by_shape.jux`, `examples/runtime_stack_overflow_crate_thread.jux`, `bin/jux/tests/runtime_failures.rs`.

| Probe | Before | Now |
|---|---|---|
| `catch (AccessDeniedException e)` around a crate write whose error wraps a `PermissionDenied` `io::Error` | `LibraryException` | caught |
| a crate's `FromStr` error of a `Decimal` / of a `Version` | `LibraryException` | `NumberFormatException` / `IllegalArgumentException` |
| a crate's `enum LookupError { KeyNotFound(..), Poisoned }` | `LibraryException` | `NoSuchElementException` / `LibraryException` |
| `print(parse("x1"))` for a `Result<int, ParseIntError>` | `Err(ParseIntError(kind: InvalidDigit))` | `Err(NumberFormatException: invalid digit found in string)` |
| `print($"${r}")` for a `Result<String, rust.std.Error>` | `<Result>` | `Err(FileNotFoundException: ...)` |
| `Module.describe_parse_error(e)` taking naga-shaped `front::wgsl::Error` | the signature took the first `Error` | `&rust.sbfix.front.wgsl.Error` |
| `rust.sbfix.front.spv.parse(words)`, `rust.sbfix.front.spv.Error.Truncated` | E0900 | work |
| a lambda recursing forever on a `std::thread::Builder` thread (Linux) | `thread 'library' has overflowed its stack`, status 134 | `panic: stack overflow: ...`, status 101 |

---

## 4. Three streams stopped mid-flight

Update 2026-09-25: all three are now committed on their worktree branches
(`worktree-agent-<id>`), so deleting a worktree directory no longer loses
work. Originally their work was uncommitted in their worktrees under
`.claude/worktrees/`. Each
can be resumed or re-derived from the notes above. If the worktrees are deleted
for space, this work is lost and must be redone; nothing of it is on a branch.

**`agent-ab1eca9d85b817d03` (gaps 3 and 4).** Merged as `770de4b`. The gate
found that `jux new` / `jux init` wrote the directory name verbatim (so
`jux init` in `clean-init/` produced an `E0903` manifest) and that 100 of the
102 in-repo manifests lacked `edition`; both fixed. By the owner's decision a
single-segment `name` is clean and `W0903` covers only a dotted name with `_`
in its root segment. Gate: 1553 passed, the four failures were those two
causes and pass on the fixed build, clippy clean.

**`agent-a0f7bb435c16f7698` (gap 5).** Gated and merged as `8c47abb`: 1540
tests passed, the two failures (`dashboard`, `svg_studio`) were crates.io
download errors and pass on rerun, clippy clean. The worktree can go.

**`agent-aaba5147cd3b13715` (gaps 1, 9 and 10).** Merged as `dc929a7`. The
first gate failed 19 tests, all of them assertions on the old
`#[derive(Debug, ...)]` lists or whole-crate searches for `return` /
`.to_string()` that the new prelude helper tripped; the helper was rewritten
as one expression and the derive assertions now check for the written
`Debug` impl. `examples/nullable_type_arguments.jux` and
`tests/ui/foreign_record_array_default.jux` pin the three gaps. Second gate:
1551 passed, 0 failed, clippy clean.

---

## 5. Operational notes worth keeping

**Seed a new worktree's `target/release` from the main checkout.** Measured
2026-09-25: 130 seconds instead of a roughly ten-minute cold build, because
cargo reuses all 134 third-party dependency rlibs and rebuilds only the juxc
crates. Fingerprints survive the path change. Costs about 1.1 GB per worktree.
Do NOT instead point several worktrees at one shared `CARGO_TARGET_DIR`: their
sources differ, so they invalidate each other's artifacts and thrash.

**Run the tests for the feature you touched, not the whole suite.** The full
gate belongs at merge and push, once per wave. The exact-output corpus builds
and runs all 411 examples as separate cargo crates and is the single biggest
lever in the suite.

**Reclaiming disk, in order:** a merged worktree's whole directory; a live
worktree's `target/debug` and probe dirs; then `target/it-*`, `target/lessons`,
`target/apps` in the main checkout. The last of those is the corpus's warm
cache, so losing it costs one slow corpus run (roughly six times slower), but
running out of disk costs failed builds and spurious link errors, which happened
three times in one session.

**CRLF.** Every `.rs` and `.md` in the repo is CRLF, and
`crates/juxc-tycheck/src/check.rs` carries exactly ONE lone `\r\r\n` near line
199 at the `"rotateLeft",` line. Both `sed -i` AND the Edit tool silently
destroy it, after which git reports the file as a 36,000-line whole-file diff.
Patch it with byte-exact python that asserts a match count, and verify after
every edit.

**`JUX_BLESS=1` rewrites every `.expected` to LF with no content change**, so a
blessing run shows 174+ files as modified. Restore with
`git checkout -- tests/ui` and hand-write only the genuinely new expectations.

**Pre-assign ERRATA numbers when running parallel streams.** Letting each take
"the next free number" cost three separate renumbering passes in one session.

**The shared stub cache** at `%LOCALAPPDATA%\juxc\stubs\rust-std.jux.d` is
contended between checkouts on one machine and produced two contradictory
readings minutes apart. Point `LOCALAPPDATA` at a private directory for a gate
run, or give the cache path the compiler's build id.

---

## 6. How to re-measure

Re-run the completeness audit the way the 76% was produced: spec by spec through
all 30 `Architecture/*.md` documents, every feature classified IMPLEMENTED /
PARTIAL / MISSING / DEFERRED-BY-OWNER, owner-deferred excluded from the
denominator, and every judgement backed by a probe program run against the built
toolchain rather than by reading code. The lesson that produced that rule: a
`ref` feature marked "fully implemented" from reading code turned out to have
six bugs, two of them silently wrong answers, and the plainest spelling of the
feature was the one that was broken.
