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

**34. OPEN (found 2026-09-27 by the idiomatic rewrite, LEAKS L29, L39).**

- **L29: egui's `Frame` cannot be named from `rust.eframe`.** `eframe::Frame`
  and `egui::Frame` share a name within one crate family, and the host's item
  wins (`FamilyPaths::rank`). So `egui::Frame` is dropped, and
  `Panel.frame(..)`, `CentralPanel.frame(..)`, `TextEdit.frame(..)` and
  `Ui.dnd_drop_zone(..)` name eframe's type. A panel cannot get its own fill
  or margins. The fix needs a second Jux name for the losing type (a nested
  package or a renamed type).
- **L39: deprecated crate methods are not marked.** A stub does not mark them
  (`Panel.show_inside`, renamed `show` in egui 0.36), and only rustc warns,
  under `--verbose`.

**35. CLOSED 2026-09-27 (branch `leaker-idiomatic`, LEAKS L30-L38).** ~~The shapes the idiomatic rewrite hit.~~

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
