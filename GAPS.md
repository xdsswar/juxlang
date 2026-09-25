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
| E99 | Recorded OPEN. See section 3, item 1. |
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

**1. `Box<int?>` leaks `Option<isize>: Display`.** Contradicts §T.2.1's explicit
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

**2. `Cell<File>` over a Jux generic.** The blanket `Clone + Debug + 'static`
on every generic declaration. Dropping it wholesale breaks the core library
(`Iterable.reduce<U>` clones its accumulator; `LazyIterable.chain` needs
`T: Clone`); dropping it only from the struct emitter leaves the inherent
`impl` bounded, so `Cell::<File>::new` stays unreachable. Needs per-parameter
bound inference in the style of the existing `hash_key_params` /
`eq_bound_params` passes. `Vec<File>` works, since a foreign generic carries
its own impls.

**3. `jux.toml` is effectively unvalidated.** `name = "MyApp"`, a missing
`version`, a missing `edition`, `edition = "2015"` and unknown tables all pass
`jux check`. Malformed TOML is an `eprintln!` warning and a `None` return
(`crates/juxc-driver/src/manifest.rs:470`), so a caller cannot tell "no
manifest" from "bad manifest". §B.2.2 marks all three keys REQUIRED, pins the
name to `^[a-z][a-z0-9]*(\.[a-z][a-z0-9_]*)+$`, and allows only edition
`"2026"`. No manifest diagnostic code exists; one has to be allocated.

**4. A dependency's entry files leak into the dependent.**
`collect_dependency_sources` (`crates/juxc-driver/src/project.rs:836`) loads a
dependency's whole `src/` tree including entry files, so a package that
path-depends on a package having any `[[bin]]` gets the dependency's `main` and
fails with `E0400`. The one-call fix is NOT safe: `cmd_doc` and `cmd_doctest`
call the same function on their own package, so filtering would silently drop
`src/main.jux` items from generated docs. The two uses must be separated first.
`load_src_tree_without_entries(manifest, keep)` from the E103 work is the filter
to reuse.

**5. Index and `@RustRefOut` reads through an alias import.** Same family as
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

**6. `rust.std.File.open("x")` as a fully-qualified static call** emits the raw
`Result` into a `rust.std.File?` slot (`E0308`). The imported spelling
`File.open(..)` is correct.

### Missing surface

**7. `Task.completed`, `Task.failed`, `map`, `flatMap`, `isCancelled`,
`isResolved`.** Listed in §18.1.4, unimplemented. They report honestly as
`E0413` now rather than leaking, so this is completing a documented surface.

**8. A task used in two combinators leaks `E0382`.** Awaiting consumes a task,
so `Task.any(a, b)` then `Task.allSettled(a, b)` is a use-after-move that rustc
reports. Needs a Jux diagnostic. Alongside it, an exception inside a `Result`
renders its Debug internals (`Err(Exception { __parent: ... })`) rather than its
message.

**9. The checker does not consult `@RustDefault`.**
`juxc_tycheck::defaults::user_type_has_default` returns `true` for any external
class, which E97's markers make false, so `new Holder[3]` over a record that
lost `Default` is reported by rustc. About five lines, but it changes
`member_has_default` for every foreign type at once, so it wants the corpus as
its guard.

**10. Enums are not covered by the derive rule.** A Jux `enum` variant holding a
non-`Clone` foreign payload still goes through `decls/enums.rs`'s unconditional
derive. E97's fix applied at a third site; `ForeignDerives` and
`foreign_derives_of` already exist.

**11. Reading a field through a bound that names a GENERIC class**
(`? extends Container<int>` then `it.someField`). Scoped into E100 rule 1
deliberately: the accessor bodies read through the shared handle at a fixed
depth, and a generic class's inherited field types are spelled in its parent's
type-parameter vocabulary, which the bound surface does not substitute through.
Calling a METHOD through such a bound does work.

**12. Record components take no annotations.** Grammar §A.2.5 is
`record-component = annotation* type identifier`. A component becomes a
canonical-constructor parameter, so it is the natural next position after E104,
but it is its own AST node with its own construction sites.

**13. Lambda parameter annotations cascade.** `(@Tag int a) -> a` produces five
errors rather than one clean diagnostic. The grammar is right to refuse them
(§A.2.9 `lambda-param = type? identifier`), so this is refusal quality. The fix
is in `parse_lambda`'s lookahead (`crates/juxc-parse/src/exprs.rs:1855`), which
does not confirm a lambda at all when the parens open with `@`.

**14. `check_annotation_applications` does not recurse into `nested_types`.**
Pre-existing: a nested class's own `TYPE`/`METHOD`/`FIELD` annotations were
never checked either. Adding the recursion would newly fire `E0470`/`E0472`/
`E0473` across the corpus, so it needs its own change and its own corpus run.

### Build, tooling, diagnostics

**15. Linking and target selection leak raw rustc and cargo.** An unlinkable
`@extern(lib="c")` dumps the whole `link.exe` command line; an uninstalled
`--target` leaks `E0463` across seven crates; an unused
`[ffi.*] linkage="framework"` entry kills the build. The rustc-leak remapper
already exists for the compile path and is the model.

**16. No lint configuration and no `-Werror` (§D.5.4).** W0820 is additionally
dropped by `jux check`, so "deny unjustified unsafe" cannot be expressed at all.

**17. `secondary_spans` and `code_action` have zero producers.** The schema and
both renderers support them; the only `.with_label` call sites in `crates/` are
LSP test fixtures. Wiring roughly ten diagnostics would deliver most of
§D.1.3's promised quality for little work.

**18. Diagnostic polish.** `E0464`'s sibling message at
`crates/juxc-tycheck/src/check.rs:6052` still says "Drop `final`/`const`" rather
than echoing the written keyword, now that `FinalKw` exists to ask. `E0203`
covers only the four non-escapable Rust words, and `E0305`'s text claims "every
other reserved word is fine", which is false.

**19. The IntelliJ plugin has no E0464 reassignment inspection at all**, so the
`final` for-each binder's meaning is parsed and recorded but nothing in the
editor consumes it.

**20. `JuxNamesValidator.isIdentifier` rejects any keyword**, so the platform
Rename dialog still refuses renaming a package segment to `type` or `record`,
although ERRATA E78 makes the resulting path legal.

**21. `tools/java-differential/run.sh` does not honour a shared
`CARGO_TARGET_DIR`** when locating the built compiler; it needs `JUX_BIN`.

### Deferred by decision, not by oversight

**22. A VS Code extension.** `juxc-lsp` answers 16 LSP methods correctly and no
VS Code user can reach any of it, which contradicts the "one server, many
editors" architecture. New surface rather than a fix. The TextMate grammar has
also drifted (`ref`, `typeof`, `weak` unhighlighted) and `jux.tmbundle/` has no
`Syntaxes/`, so the documented install path does not load.

**23. The §CR representation selector is unbuilt.** Every class is
`Rc<RefCell>`, including the spec's own "zero heap allocation" example. This is
the single largest scoring hole (§CR at 44%), but it is a performance promise,
not correctness, and §CR.9 sequences it as a later phase. It also means
`E0950`-`E0952` are unreachable and should be marked so.

**24. User-package-to-user-package bare-name capture.** A bare name in
`package a;` can still reach a type in an unrelated `package b;` through the
cross-package fallback. Left deliberately in E96: it is what makes `E0416` fire
at all for user code. §M.16's ladder is written so tightening it later is a
refinement, not a contradiction.

**25. `const_eval`'s constant lookup is still name-keyed** (`ConstCtx` carries
no package). Cannot bite today because `jux.std` declares no constants; if one
is ever added, `ConstCtx` needs a package.

---

## 4. Three streams stopped mid-flight

Their work is uncommitted in their worktrees under `.claude/worktrees/`. Each
can be resumed or re-derived from the notes above. If the worktrees are deleted
for space, this work is lost and must be redone; nothing of it is on a branch.

**`agent-ab1eca9d85b817d03` (gaps 3 and 4, manifest validation).** Had written
ERRATA, the build-system addendum, the diagnostics addendum, a new
`crates/juxc-driver/src/manifest_check.rs`, and edits to `manifest.rs`,
`project.rs`, `lib.rs`, `bin/jux/src/main.rs` and `multi_bin_project.rs`. Was
flattening a multi-line TOML parse message when stopped. (It also left a stray
`jt.txt` and `probe/` in the worktree.)

**`agent-a0f7bb435c16f7698` (gap 5, alias index and ref-out).** Had edits to
`exprs/array.rs`, `exprs/mod.rs`, `stmts.rs`, the missing-defs addendum and
ERRATA, plus a new `examples/stdlib_alias_reads.jux` with a hand-written
expectation. Was entering its gate when stopped, so it is the closest to done.

**`agent-aaba5147cd3b13715` (gaps 1, 9 and 10).** The largest: edits across
`decls/{classes,enums,functions,interfaces,operators,records}.rs`,
`analysis.rs`, `types.rs`, `lib.rs` and `juxc-tycheck/src/defaults.rs`. Was
gating the enum's `cases()` helper on `Clone` and fixing the tycheck default
when stopped. This is the `Debug`-canonical change, so it is the one that most
needs the full corpus run before it can be trusted.

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
