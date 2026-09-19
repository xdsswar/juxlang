# Jux lessons

67 short Jux teaching programs, one concept each: Hello, Variable, Const,
and so on up to allocators and inline assembly. They serve two purposes at
once:

- **Teaching material.** Each `src/Main.jux` is commented as a lesson: what
  the construct is, why it exists, and the trap worth knowing before relying
  on it.
- **A test corpus.** Each lesson has an `expected.txt` with its exact output,
  and `bin/jux/tests/lessons.rs` builds, runs and compares every one of them.

## Layout

```
tests/lessons/
  README.md              this file: conventions and the results table
  known-failures.txt     lessons that fail today, one `Name: reason` per line
  <Name>/jux.toml        the project manifest (the only manifest; no module.jux)
  <Name>/src/Main.jux    the lesson (Module has more files, in packages)
  <Name>/expected.txt    exact stdout
  <Name>/stdin.txt       optional scripted input, piped to the program
```

## Running

```
cargo test -p jux --test lessons                   # every lesson
LESSON=Hello,Range cargo test -p jux --test lessons   # just these
```

The test discovers lesson folders from this directory, copies each one to
`target/lessons/<Name>` so the build never writes into the sources, runs
`jux run --manifest-path` on the copy, and compares stdout with
`expected.txt`. Both sides are compared after unifying line endings, dropping
trailing whitespace on every line, removing `jux:` status lines, and trimming
leading and trailing blank lines. Everything else must match exactly.

All failures are reported together in one message. A lesson listed in
`known-failures.txt` may fail; a listed lesson that starts passing fails the
test with "remove it from known-failures", so a fixed bug cannot hide behind a
stale entry.

## Conventions

Every lesson is written the same way.

- The entry point is `void main()` (or `int main()` where the exit code
  matters); output goes through `print($"x ${v}")`.
- Locals that never change are `final`; `switch` uses `case ... ->` arms;
  "maybe a value" is `T?`; `Result<T, E>` and `expr?` follow Core lib §K.4,
  with exceptions shown where they are the better Jux answer.
- Collections are the Rust std ones under their Rust names (`Vec`,
  `VecDeque`, `HashMap`, `HashSet`, `BTreeMap`) with Rust method names (`push`,
  `len`, `contains`, `sort_unstable`); the full profile uses the global heap.
- Equality, ordering, hashing and text are operator overrides
  (`operator==`, `operator<=>`, `operator hash`, `operator string`), never
  methods.
- Data-carrying cases are an `enum` with payloads; behaviour lives inside the
  type; `struct` is a value type.
- A function over a sequence takes a Jux array `T[]` (any length) or a
  `Vec<T>`; Jux arrays and `Vec` are reference types.
- `Variadic` uses `T... name` (sugar for `T[]`), and `any...` for mixed types.
- `Module` is a multi-package project: one `jux.toml`, the entry point in
  `src/Main.jux`, and each package in a directory that mirrors its name.
- Libraries Rust's std lacks come from crates: Json uses `rust.serde_json`;
  Random and Password use `rust.rand` + `rust.rand_pcg` with a fixed seed;
  Age and Time use `rust.chrono`; Unicode uses `rust.unicode-segmentation`;
  File, Directory, Path and Binary use `rust.std`.
- Scope cleanup is `try`/`finally` or a `drop` block; raw memory uses unsafe
  pointers with C `malloc`/`free`; target selection uses `@cfg`. Real gaps
  (`union`, inline `asm`, arena and custom allocators, compiler-version
  queries) are written as far as Jux allows and marked GAP.

## Jux semantics the transcripts show

Each `expected.txt` is what Jux prints. Worth knowing when reading them:

- **Floating point text.** A `double` prints like Java's `Double.toString`
  and a `float` like `Float.toString`: `7.0` rather than `7`, `3.0 x 4.0`
  rather than `3 x 4`, and `3.1415927` for a `float` pi. (Convert, Struct,
  Tuple, Variant, Interface, TypeAlias, Math, Statistics.)
- **Ties round to even.** `round()` and `toFixed(n)` break an exact tie
  towards the even digit: `(-2.5).round()` is `-2.0` and `0.125.toFixed(2)`
  is `0.12`. (Math, Format.)
- **No placeholder specs.** Jux interpolation has no `{:>8}` or `{:08b}`.
  Bases come from `toBinary()`, `toOctal()`, `toHex()`; precision from
  `toFixed(n)`; width, fill and alignment from a few lines of helper code in
  the lesson. (Primitive, Operator, Convert, Format, Console, Variant,
  Propagate, Overloading, Algorithm.)
- **One `print`.** `print` always ends the line; a line built from pieces is
  assembled into a `String` and printed whole, so trailing spaces are not
  significant. (Loop, Range, Console, Thanks, Prime, Statistics, Algorithm.)
- **One `bool` and one `char`.** C widths are spelled at the FFI boundary.
  (Primitive.)
- **Every `switch` is exhaustive.** A number that fits no arm is error
  E0440, and silence needs an explicit empty `default`. (Match.)
- **Operators come in consistent sets.** `operator==` requires
  `operator hash` (E0931), and ordering is `operator<=>` or all four of
  `<`, `<=`, `>`, `>=` (E0930). (Interface, Overloading.)
- **Literals and enums.** An integer literal past `long.MAX_VALUE` needs the
  `uL` suffix; an enum with chosen numbers is a `@layout(c, repr = "u8")`
  enum with every value written, and a plain enum gives `ordinal()`.
  (Primitive, Enum.)
- **Strings have two lengths.** `charLength()` and `byteLength()`, with no
  byte indexing that returns a `char`. (String, Slice.)
- **No array slices.** Jux has no `numbers[1..4]` view; the Slice lesson
  builds the view as a record holding the array reference and the bounds.
- **No `move` yet.** Jux reserves `move` but Phase 1 does not implement it;
  classes are shared by reference counting, so the Ownership lesson shows
  sharing, with a `drop` block that runs exactly once.
- **Foreign libraries answer in their own terms.** serde_json reports a
  line and column, a `Vec` grows by Rust's policy (capacity 0, then 4, then
  8), and the Unicode lesson asks `isDigit()`. (Json, Vector, Unicode)
- **Nothing depends on the machine or the clock.** Elapsed time, the path
  separator and an OS-seeded draw print only what is certain about them
  (`at least 20 ms: true`); files live under the system temp directory;
  a directory listing is sorted before it is shown. (Time, Path, Random,
  File, Directory, Binary)

## Results

Wave 1 (the 39 direct lessons) was run on 2026-09-18 against the worktree's
own `jux`. Status: **PASS** (exact output), **FAIL** (compiler or std bug,
listed in `known-failures.txt`, repro below), **GAP** (a real language or
library gap, listed too), **TODO** (a later wave).

| Lesson | Wave | Status | Notes |
|---|---|---|---|
| Hello | 1 | PASS | |
| Variable | 1 | PASS | |
| Const | 1 | PASS | |
| Primitive | 1 | PASS | `18446744073709551615uL` needs the suffix; one bool/char width |
| Convert | 1 | PASS | `int->float` prints `7.0` |
| Operator | 1 | PASS | binary via `toBinary()` + padding |
| Condition | 1 | PASS | parentheses are required in Jux |
| Ternary | 1 | PASS | a text conditional goes straight into interpolation |
| Loop | 1 | PASS | `loop` becomes `while (true)` |
| Range | 1 | PASS | overflow panics in debug instead of wrapping |
| Function | 1 | PASS | |
| Overload | 1 | PASS | |
| Callback | 1 | FIXED | B1: a free function passed as a value leaked rustc E0308 (7cc5510) |
| Generic | 1 | PASS | `where T has operator<=>(T) -> int` bound |
| Variadic | 1 | PASS | `any...` for mixed types, `int...` accepts an array |
| Array | 1 | PASS | |
| Slice | 1 | FIXED | B3, B4, B5 (record static call, user `length()`, array component) (2d432c5); no slice syntax, view built by hand |
| Tuple | 1 | PASS | |
| Struct | 1 | PASS | |
| Enum | 1 | PASS | C-layout enum for discriminants, `ordinal()` for plain |
| Variant | 1 | PASS | enum payloads; printing derives `Range(low: 20.0, high: 24.0)` |
| Match | 1 | PASS | exhaustive switch; `case 6, 7 ->` added |
| Option | 1 | PASS | `int?`, null narrowing, `?:` |
| Result | 1 | FIXED | B2: switching on a `Result` leaked rustc E0308 (182aa5c) |
| Propagate | 1 | FIXED | B2 again, plus a moved `String` argument inside `f(x)?` (182aa5c) |
| TypeAlias | 1 | PASS | `{...}` under an array alias works since 4ae3b24 (B13); the lesson still writes `new T[]` |
| Interface | 1 | PASS | `operator==` needs `operator hash` |
| Iterator | 1 | PASS | `Iterator<T>`/`Iterable<T>` classes |
| Overloading | 1 | PASS | `operator<=>` instead of a lone `<` |
| Ownership | 1 | PASS | `move` not implemented; shows sharing plus `drop` |
| String | 1 | PASS | `substringBytes` added in 905442a (B11); the lesson still tests the boundary on bytes |
| Format | 1 | PASS | helpers instead of specs; `0.125` ties to `0.12` |
| Math | 1 | FIXED | B14: Rust `f64` methods reach `double` (8acb573) |
| Prime | 1 | PASS | `Vec<bool>` |
| Statistics | 1 | FIXED | B8: runtime panic, `for (i : 1..v.len())` held the Vec borrowed (d6b2b91) |
| Algorithm | 1 | FIXED | B6, B7 (d6b2b91, fc4b7c8); iterator adaptors reachable since 8acb573 (B14); `sort` is not discoverable, `sort_unstable` is |
| Console | 1 | PASS | |
| Thanks | 1 | PASS | |
| Module | 1 | PASS | three packages; package-private leak found (B9, now E0416 since 9065573) |
| Vector | 2 | PASS | `Vec`; `get` gives `int?` (B16, typed directly since 3e6be67); capacity follows Rust (0, then 4, then 8) |
| Deque | 2 | PASS | `VecDeque`; `pop_front`/`pop_back` give `int?` |
| HashMap | 2 | PASS | `HashMap`/`HashSet`; a set's `insert` returns whether the value was new |
| TreeMap | 2 | PASS | `BTreeMap` |
| Json | 2 | FIXED | B17 (3c33bd6), B18 (991b1ef, 4ad77b3); `Value` is switched on as an enum |
| Random | 2 | FIXED | B20, B22 (36bc3b2), B21 (4c3144f); expected output from a Rust oracle on rand 0.9.5 + rand_pcg 0.9.0 |
| Password | 2 | FIXED | B22 (36bc3b2); a fixed seed keeps the output checkable, and the lesson says why that is wrong |
| Unicode | 2 | FIXED | B23 (36bc3b2), B24 (a94b0f2) |
| Age | 2 | PASS | chrono `NaiveDate`; the age is a record |
| Time | 2 | FIXED | B25 (96110d8), B26 (9832be2) |
| File | 2 | FIXED | B27 (3c33bd6); temp dir, `File.create`/`open`, `write`/`read` byte counts |
| Directory | 2 | FIXED | B28 (3e6be67), B29 (29be156); names are sorted, since listing order is the file system's |
| Path | 2 | FIXED | B21 (4c3144f), B30, B31 (29be156); `Path.is_empty()` is unstable, so not in rust.std (B32, f293812) |
| Binary | 2 | PASS | byte order by shifts (`to_le_bytes` reachable since 8acb573, B14); `read` typed directly since 3c33bd6 (B33) |
| Circle | 3 | FIXED | B40 (e609af8), B41 (f5dacbf) |
| Guess | 3 | FIXED | B40 (e609af8); seeded `StepRng`; `Pcg64.seed_from_u64` works too since 36bc3b2 (B43) |
| Launch | 3 | PASS | pauses divided by 100; sleeps with a `Duration` since 96110d8 (B44) |
| Quadratic | 3 | FIXED | B40 (e609af8); bad input is a catchable exception since 1c3483c (B42) |
| Melody | 3 | PASS | prints the notes; `Beep` only under `--features sound` on Windows |
| Defer | 3 | PASS | nested `try`/`finally` for ordering, a `drop` block for the release |
| Memory | 3 | PASS | C `malloc`/`free` through FFI (Jux has no `delete`, E0507); alignment printed, not the address |
| Pointer | 3 | PASS | one pointer type `T*`, no read-only form; `unsafe public` parses since 9274da2 (B45) |
| Config | 3 | PASS | `@cfg` / `if cfg`; no `#source` query (gap), function name passed by hand |
| Version | 3 | GAP | no compiler-version query; stand-in version record ordered with `<=>` |
| Extern | 3 | FIXED | B46: a String passed to a native function is no longer moved (c6d09e2) |
| Union | 3 | GAP | no `union`; stand-in rereads one `i32` through cast pointers |
| Asm | 3 | GAP | no inline `asm` (E0301); bodies in Jux, `@cfg(arch)` selection kept |
| Allocator | 3 | GAP | no allocator parameter or arena; the arena strategy built by hand over a `Vec` |

Wave 1 totals: 32 PASS, 7 FIXED (were FAIL or GAP), 0 GAP.

Wave 2 (collections, crates and std I/O, 14 lessons) was run on 2026-09-18:
6 PASS, 8 FAIL. Every failure is a compiler or binder bug with a repro below;
none is a gap in the language itself. All eight are FIXED now: 6 PASS, 8 FIXED.

## Bugs found by wave 1

Each is a minimal program that shows the problem on its own. "Leak" means
juxc accepted the program and rustc rejected the Rust it emitted.

**B1. A free function used as a value leaks E0308.** (Callback)

```jux
int twice(int v) { return v * 2; }
int apply((int) -> int f, int x) { return f(x); }
void main() {
    print(apply(twice, 3));        // rustc: expected Rc<dyn Fn(isize) -> isize>, found fn item
    (int) -> int g = twice;        // same
}
```

Status: FIXED in 7cc5510.

**B2. `switch` on a `Result` lowers to Rust's own `Result`.** (Result,
Propagate) The patterns are emitted as `Result::Ok(v)`, which rustc resolves
to `std::result::Result`, not `jux::std::result::Result`. A nested switch on
the bound error then loses its enum path (`NotExact(..)` not found, E0531).

```jux
Result<int, String> half(int n) {
    if (n % 2 != 0) { return Result.Err("odd"); }
    return Result.Ok(n / 2);
}
void main() {
    switch (half(4)) {
        case Ok(var v) -> print(v);
        case Err(var e) -> print(e);
    }
}
```

Status: FIXED in 182aa5c.

**B3. A static method call on a record leaks E0423.** (Slice)

```jux
record P(int x) {
    static P zero() { return new P(0); }
}
void main() { print(P.zero().x); }   // emitted as `P.zero()`, not `P::zero()`
```

Status: FIXED in 2d432c5.

**B4. A user method named `length()` is rewritten to the length intrinsic.**
(Slice) Happens on records and classes alike; the emitted
`w.len() as isize()` does not parse (E0214) and `len` does not exist (E0599).

```jux
class C {
    int n = 2;
    int length() { return n; }
}
void main() { print(new C().length()); }
```

Status: FIXED in 2d432c5.

**B5. Indexing an array component inside a record leaks E0608.** (Slice)

```jux
record A(int[] items) {
    int first() { return items[0]; }   // self.items[0] on Rc<JuxCell<Vec<isize>>>
}
void main() { final int[] xs = {4, 5}; print(new A(xs).first()); }
```

Status: FIXED in 2d432c5.

**B6. A range bounded by `Vec.len()` binds a usize.** (Algorithm) The loop
variable's type leaks into an `int` assignment or an `int?` return.

```jux
int lastIndex(Vec<int> items) {
    var best = 0;
    for (var i : 0..items.len()) {
        best = i;                     // rustc: expected isize, found usize
    }
    return best;
}
```

Status: FIXED in d6b2b91 (spec M.6.1 now types the range by its bounds: `0..v.len()` is a `uint` range) and fc4b7c8 (a `uint` returned into `int?`).

**B7. A nullable narrowed inside a ternary branch leaks when interpolated.**
(Algorithm) The narrowed value is unwrapped and then matched as an `Option`
again.

```jux
int? find(int x) { if (x > 0) { return x; } return null; }
void main() {
    final int? n = find(3);
    print(n != null ? $"found ${n}" : "none");   // match &(n.unwrap()) { Some(..) }
}
```

Status: FIXED in fc4b7c8.

**B8. A `for` over `0..v.len()` keeps the Vec borrowed for the whole loop.**
(Statistics) Writing an element inside the loop panics at run time with
"RefCell already borrowed". This one compiles cleanly and fails only when run.

```jux
void main() {
    var v = new Vec<int>();
    v.push(1);
    v.push(2);
    for (var i : 0..v.len()) {
        v[i] = 7;                     // panic: RefCell already borrowed
    }
    print(v[0]);
}
```

Status: FIXED in d6b2b91.

**B9. A package-private free function is visible from another package.**
(Module) Language §4.4 makes a declaration with no modifier visible within its
package only, and reaching it from elsewhere E0416. In the Module project,
`src/Main.jux` can `import lessons.module.geometry.doubled;` and call
`doubled(5)`, though `doubled` has no modifier; it compiles and prints `10`.
The lesson follows the spec and does not rely on this.

Status: FIXED in 9065573 (E0416).

**B10. `Result.err()` is missing.** Core lib §K.4 lists `Option<E> err()`;
`r.err()` is E0413 "no method `err` on type `jux.std.result.Result`".

Status: FIXED in b2b6cd0.

**B11. `String.substringBytes` is missing.** Core lib §K.7 lists
`substringBytes(int, int) throws EncodingException, IndexOutOfBoundsException`;
calling it is E0413. The String lesson tests the boundary on `bytes()`
instead.

Status: FIXED in 905442a.

**B12. An error inside `${...}` on a parenthesized receiver is reported at
1:1.** `print($"${(2.0).nosuch()}");` gives
`file.jux:1:1: [E0413] no method nosuch on double`; the same call without
the parentheses reports the right line and column.

Status: FIXED in 5f120f6. The cause was the literal receiver, not the interpolation: `(2.0).nosuch()` was reported at 1:1 anywhere.

**B13. `{...}` under an array type alias is a parse error.**
`type Bytes = ubyte[]; final Bytes data = {1, 2, 3};` gives three E0200
"expected expression" errors and a follow-on E0601. `new ubyte[] {1, 2, 3}`
works. Either the initializer should be accepted or the message should say
that a `{...}` initializer needs the array type written out.

Status: FIXED in 4ae3b24 for an alias declared in the same file, and in 46fc6ca for one declared in another file of the build; the E0601 that followed an unreadable `{...}` is gone too (46fc6ca). A `uint` into an `int?` slot converts since b980847.

**B14. Parts of the Rust std are not reachable.** (Math GAP, Algorithm,
Statistics) The rule is that foreign APIs are discovered from rustdoc, so
each of these is a discovery gap rather than a list to extend by hand:
- `double` has no Rust `f64` methods: `powf`, `ln`, `log2`, `log10`, `exp`,
  `sin`, `cos`, `tan`, `hypot`, `trunc`, `min`, `max` are all E0413.
- `import rust.std.f64.consts.PI;` is E0301.
- `Vec` has `sort_unstable` and `sort_unstable_by` but not `sort` or
  `sort_by` (both live on `[T]` in `alloc`).
- `rust.std.Iter` has none of the `Iterator` trait adaptors: `filter`,
  `position`, `sum`, `fold`, `min` are E0413.
- There is no way to sort a `Vec<double>`: `total_cmp` is missing and an
  ordering closure cannot produce a Rust `Ordering`.

Status: FIXED in 8acb573. Every integer, float, `char` and `bool` has Rust's methods (discovered per primitive from rustdoc), Rust's `Iterator` trait is surfaced as `RustIterator` with its adaptors, and `total_cmp` plus `sort_unstable_by` sort a `Vec<double>`. One part is the toolchain's: the prebuilt `alloc` rustdoc JSON omits `impl<T> [T]`, so `sort`/`sort_by` are not discoverable (Bindgen G.6.4.4). `import rust.std.f64.consts.PI` stays E0301: `rust.std` is one flat package and has no module path to import through.

**B15. Smaller tooling and wording issues.**
- `jux run` in project mode (`--manifest-path`) ignores `--emit-dir` and
  builds under `<project>/target`, and its help still says project mode is
  "not yet implemented". The test stages each lesson under `target/` for this
  reason.
- `String.length()` is accepted and counts characters, although Core lib §K.7
  says a `String` has no bare `length` (only `charLength()` and
  `byteLength()`). The lessons use the spec names.
- E0931 on a struct says "class `Circle` defines `operator==` but no
  `operator hash`".

Status: `--emit-dir` in project mode and the help text FIXED in 177cdcc; E0931 wording FIXED in 2966922. `String.length()` is KEPT by owner ruling (2026-09-19): it counts characters like `charLength()`, and ERRATA E13 records it.

## Bugs found by wave 2

Wave 2 is the first wave to lean on the foreign boundary: Rust std types used
by value, three crates.io crates, and `std::fs`. Most of what it found is in
that boundary, in the binder (what a stub says) or in the lowering of calls
into and results out of foreign code. "Leak" means the same as above.

Where a lesson could keep its natural shape with a small change, it does and
passes, with the change and the bug named in a comment (Vector, Binary).
Everywhere else the lesson keeps the code a Jux programmer would write and is
listed in `known-failures.txt`. The Random and Password expected output comes
from a small Rust program using the same crate versions (rand 0.9.5,
rand_pcg 0.9.0) and the same `i64` ranges, since those numbers cannot be
derived by reading.

**B16. `Vec.get` is typed `Output?`.** (Vector) The result of `get` comes
from `SliceIndex::Output`, which the checker leaves unresolved, so spelling
the type out is refused. `var first = v.get(0);` works and prints correctly.

```jux
void main() {
    var v = new Vec<int>();
    v.push(4);
    final int? first = v.get(0);   // E0410: expected int?, found rust.std.Output?
    print(first ?: -1);
}
```

Status: FIXED in 3e6be67 (a projection is an unknown type, not `std::process::Output`).

**B17. `Option<()>` is stubbed as `void?`, which does not parse.** (Json)
serde_json's `Value::as_null` returns `Option<()>`, and the generated stub
says `public void? as_null();`. The parser stops there (E0200 "expected
identifier" in `serde_json.jux.d`), so no program can use `rust.serde_json` at
all. With that one line deleted by hand, the rest of the lesson compiles up to
B18.

```toml
[dependencies]
"rust.serde_json" = "1"      # any program importing rust.serde_json.Value
```

Status: FIXED in 3c33bd6 (a member with no Jux spelling is left out).

**B18. A for-each over a map yields references.** (Json) Iterating a
`HashMap` (or serde_json's `Map`) binds each entry as a tuple of `&K, &V`, and
passing its fields on leaks E0308. `members.get(name)` inside such a loop
leaks too (`Borrow<&String>` not implemented).

```jux
import rust.std.HashMap;
void show(String name, int value) { print($"${name}=${value}"); }
void main() {
    var ages = new HashMap<String, int>();
    ages.insert("ada", 36);
    for (var entry : ages) {
        show(entry.0, entry.1);    // rustc: expected String, found &String
    }
}
```

Status: FIXED in 991b1ef (a foreign map walks owned entries); a collection bound from a foreign enum payload is a handle since 4ad77b3.

**B19. A stub is not regenerated when a dependency's version changes.**
Changing `"rust.rand" = "0.8"` to `"0.9"` in jux.toml kept the 0.8 stub
(`gen_range`, no `Pcg64Dxsm`) while cargo linked 0.9; changing it back kept
the 0.9 stub while cargo linked 0.8.8. Deleting `.jux-stubs/` is the only
way out. BINDGEN §G.11.2 keys staleness on the dependency version.

Status: FIXED in a976490 (the stub marker records the version requirement and features).

**B20. A static call through a crate's type alias leaks E0423.** (Random)
`Pcg64Dxsm` is `public type Pcg64Dxsm = Lcg128CmDxsm64;` in the rand_pcg stub.
A static call through the alias is accepted, even for a method the aliased
class does not have (B22), and emitted with a dot.

```jux
import rust.chrono.Duration;      // chrono: type Duration = TimeDelta
void main() {
    var d = Duration.zero();      // emitted `Duration.zero()`: rustc E0423
    print(d.is_zero());
}
```

Status: FIXED in 36bc3b2 (a crate alias of a class is that class).

**B21. Importing a foreign interface, enum or constant emits the wrong
path.** (Random, Path) A class import uses the stub's `@rust` path; the
others use the Jux package path instead.

```jux
import rust.std.Read;             // `use rust::std::Read;`   (E0433)
import rust.std.Component;        // `use rust::std::Component;`
import rust.std.MAIN_SEPARATOR;   // `use rust::std::MAIN_SEPARATOR;`
import rust.rand.SliceRandom;     // `use rand::SliceRandom;`, really rand::seq::SliceRandom
```

Status: FIXED in 4c3144f.

**B22. Trait impls from another crate, and blanket impls, are not
surfaced.** (Random, Password) rand_pcg implements rand_core's `RngCore` and
`SeedableRng` for its generators, and its stub even declares both
interfaces, but `class Lcg128CmDxsm64` has no `implements` clause. So
`seed_from_u64` is E0413 on the class, and rand's `Rng` (a blanket impl over
every `RngCore`) never reaches it: `random_range` is E0413 too. The same rule
hides `SliceRandom::shuffle`, implemented by rand for `[T]`: `v.shuffle(rng)`
is E0413 on a `Vec`. `rand::rng()` (a rand type) does get `random_range`.

```jux
import rust.rand_pcg.Lcg128CmDxsm64;
void main() {
    var g = Lcg128CmDxsm64.seed_from_u64(1);   // E0413: no static method seed_from_u64
}
```

Status: FIXED in 36bc3b2 (traits from other crates, blanket impls, impls for slices; Bindgen G.6.4.3).

**B23. A crate trait implemented for `str` is not reachable on `String`.**
(Unicode) unicode-segmentation's `UnicodeSegmentation` is implemented for
`str`; its stub declares the interface, but `"e\u{301}!".graphemes(true)` is
E0413 "no method `graphemes` on `String`".

Status: FIXED in 36bc3b2.

**B24. `char.toUppercase()` and `toLowercase()` are ASCII-only.** (Unicode)
Silent wrong result. They lower to `to_ascii_uppercase()` /
`to_ascii_lowercase()` (`juxc-backend-rust/src/exprs/call.rs`), so
`'é'.toUppercase()` is `é`, not `É`. Core lib §K.11 gives `char` (a Unicode
scalar) a `toUppercase()` returning one `char`, so the simple Unicode mapping
is expected; `'ß'` staying `ß` is right. `isDigit` is ASCII-only in the same
way.

Status: FIXED in a94b0f2 (simple Unicode case mapping, core lib K.11).

**B25. `std::time::Duration` is unreachable.** (Time) `Duration` is defined
in `core` and re-exported by std, and the `rust.std` surface leaves `core` out
(§G.6.2.1), so `import rust.std.Duration;` is E0301 and `sleep` cannot be
called. `Instant` (defined in std) works, and `elapsed()` returns a value of
the missing type.

Status: FIXED in 96110d8 (what std re-exports from core is part of rust.std).

**B26. Crate types are visible without an import.** (Time) With
`rust.chrono` in jux.toml, a bare `NaiveDate` resolves with no import at all,
and a bare `Duration` silently becomes chrono's `Duration` alias
(`TimeDelta`): the program then fails with "no method `as_secs` on
`rust.chrono.TimeDelta`", naming a type the programmer never wrote.

Status: FIXED in 9832be2 (a bound crate's types need their import).

**B27. A collection handle passed to a foreign static method leaks.** (File)
`PathBuf` and `Vec` are reference types, held as a shared handle. Passed to a
free function (`create_dir(path)`, `write(path, bytes)`) the handle is
unwrapped; passed to a static method it is not.

```jux
import rust.std.File;
import rust.std.PathBuf;
import rust.std.temp_dir;
void main() {
    final PathBuf path = temp_dir().join("b27.txt");
    var file = File.create(path);   // rustc E0277: Rc<JuxCell<PathBuf>>: AsRef<Path>
}
```

`String.from_utf8(bytes)` with a `Vec<ubyte>` local, and
`String.from_utf8_lossy(buffer)` with a `ubyte[]`, fail the same way (E0308).

Status: FIXED in 3c33bd6.

**B28. Iterating `read_dir` binds the item as `DirEntry` but emits a
`Result`.** (Directory) Each item of `std::fs::ReadDir` is an
`io::Result<DirEntry>`. juxc accepts `final DirEntry found = entry;` and
emits the Result unchanged (rustc E0308). The item should either throw like
every other `Result` from Rust (§G.5.4) or be refused.

Status: FIXED in 3e6be67 (a Result item throws, Bindgen G.6.4.2).

**B29. A nullable borrowed result of an unsized type leaks `.cloned()`.**
(Directory, Path) `to_str()` on an `OsStr`/`OsString` and `extension()` on a
path are `@RustRefOut` results of `str`/`OsStr`; `?:` and `!!` on them emit
`.cloned()` on `Option<&str>` / `Option<&OsStr>` (E0599). In the Path lesson
the `?:` fallback inside `${...}` is dropped from the emitted
`__jux_show!(name.to_str())` altogether.

```jux
import rust.std.PathBuf;
import rust.std.temp_dir;
void main() {
    final PathBuf p = temp_dir().join("abc.txt");
    final String s = p.extension()!!.to_str() ?: "?";   // rustc E0599 on .cloned()
    print(s);
}
```

Status: FIXED in 29be156.

**B30. `new Path(...)` makes an unsized local.** (Path) Rust's `Path` is
unsized and only ever used behind a reference; the local is emitted as
`let path: std::path::Path = std::path::Path::new(...)` (E0308 and E0277).

```jux
import rust.std.Path;
void main() {
    final Path path = new Path("a/b");
    print(path.is_absolute());
}
```

Status: FIXED in 29be156 (a view is held as its owned form).

**B31. A foreign enum variant pattern is emitted unqualified.** (Path)
`switch (part) { case Normal(var name) -> ... }` over `std::path::Component`
emits `Normal(name) =>` (E0531, "cannot find tuple struct or tuple variant
`Normal`"). A user enum's variant is qualified; a foreign one is not.

Status: FIXED in 29be156.

**B32. The `rust.std` stub offers unstable APIs.** The vendored stub was
generated from a nightly std and includes `Path.is_empty()`, which stable
rustc rejects (E0658 `path_is_empty`); `normalize_lexically` and others are
there too. The Path lesson asks `path.as_os_str().is_empty()` instead.

Status: FIXED in f293812 (unstable APIs are probed with the build toolchain and left out, Bindgen G.6.2.3).

**B33. A declared `Vec` from a foreign function leaks.** (Binary)
`final Vec<ubyte> raw = read(path);` emits `let raw: JuxArr<Vec<u8>> =
read(...)` and rustc rejects it (E0308). `var raw = read(path);` works, and
indexing and `len()` on it are fine.

Status: FIXED in 3c33bd6 (`Vec` keeps its name in stubs).

**B14 again.** The Rust integer methods are unreachable on primitives in the
same way as the `f64` ones: `value.to_le_bytes()` on a `u32` is E0413. The
Binary lesson does the byte order with shifts.

## Wave 3: scripted input, Jux idiom, gaps

Wave 3 (14 lessons) was run on 2026-09-18 against the release `jux`.
Wave 3 totals: 6 PASS, 4 FAIL, 4 GAP; the four FAIL lessons are FIXED now (6 PASS, 4 FIXED, 4 GAP). The FAIL lessons keep the natural
code, and each was also run with the one failing construct rewritten, so the
rest of it is known to work: every FAIL below is that one bug and nothing
else.

How the lessons map, beyond the rules above:

- **Input.** `stdin().read_line(line)` from `rust.std` returns the byte
  count, zero at end of input. A prompt is written with
  `stdout().write_all(...)` and `flush()`, so it stays on the line the answer
  is typed on. Piped input is not echoed, so the next
  output lands on the prompt's line: `Circle radius: Circumference: 15.7080`.
- **Parsing.** `text.parse<double>()` is Rust's `str::parse`. A Rust `Result`
  reaches Jux as `throws`, so a bad number is a `catch`, and a nullable
  `double?` return says both "no value" and the value. The scripted
  inputs are all valid, because the `catch` does not work yet (B42).
- **`defer`** becomes nested `try`/`finally` for statement ordering, and a
  class with a `drop` block for releasing memory (Defer).
- **`Alloc`/`Free`** become C's `malloc`/`free` declared in an `unsafe native`
  block (`msvcrt` on Windows, `c` elsewhere, picked by `@cfg`). The mapping
  rule above says `new`/`delete`, but Jux has no `delete` (`delete p;` is
  E0507 by design); memory from C goes back to C. `new` and reference
  counting are the managed heap, which needs no release at all.
- **Pointers.** Jux has one pointer type, `T*`, always writable; there is
  no read-only pointer type. `&x`, `*p` and `p[i]` need
  `unsafe`; comparing with `null` does not. Addresses change every run, so
  Memory prints the alignment instead.
- **`when` / `#target` / `#build`** become `@cfg` declarations and
  `if cfg(...)` (Config, Melody, Asm). Jux has predicates, not values, so the
  target is described by declaring one answer per case. `debug` and
  `release` also stand for the optimization mode.
- **Emoji** are written as `\u{1F680}` escapes (Launch).

### Gaps

These have no Jux spelling today. Each lesson is written as far as Jux goes,
with a stand-in for the missing part with a fixed transcript.
The GAP lessons pass with their stand-ins, so they are not in
`known-failures.txt`.

- **Compiler-version query** (Version). `@cfg` knows the target, build mode,
  profile and features, but not the version of `juxc`, and no constant
  carries it. The stand-in is a version record ordered with `<=>` and
  compared at run time, not while compiling.
- **Source-location query** (Config). Nothing like `#source.fileName`,
  `.line` or `.function`. The lesson's trace helper takes the function name
  by hand. (The Config lesson passes otherwise.)
- **`union`** (Union). `union` is not a keyword. A declaration is read as a
  function returning a type named `union`, and fails with E0417 plus E0460
  and three E0200s rather than one message saying unions are not supported.
  The stand-in rereads one `i32` through `u32*` and `ubyte*` casts inside
  `unsafe`, which is the reinterpretation a union exists for.
- **Inline assembly** (Asm). `asm("...")` inside `unsafe` is E0301, "cannot
  find `asm` in this scope". The spec reserves it for the `embedded` and
  `core` profiles and it is not implemented; there is no `asm` function form.
  The stand-in writes both functions in Jux, with the assembly in comments.
- **Allocators** (Allocator). No `Allocator` interface, no allocator
  parameter, no arena, no allocator-taking `Box<T>`: every `new` uses the one
  global heap. The stand-in builds the arena strategy by hand over a
  `Vec<long>`, which only covers values put through it on purpose.

## Bugs found by wave 3

Numbered from B40; wave 2 uses B16-B39.

**B40. `int main()` leaks E0277.** (Circle, Guess, Quadratic) Entry Points
§E lists `int main()` for a program that returns an exit code, but it is
emitted as a plain Rust `fn main() -> isize`, which rustc rejects (`main`
can only return a `Termination` type). Same in project mode and with
`juxc --run`.

```jux
int main() {
    print("hi");
    return 3;
}
```

Status: FIXED in e609af8.

**B41. A `const` on the left of a product loses its type for a method
call.** (Circle) `(2.0 * Pi * r).toFixed(2)` works; with the constant first
the call is emitted as a real Rust method `toFixed` on `f64` (E0599). Binding
the product to a `double` local first also works.

```jux
const double Pi = 3.14;
void main() {
    double r = 2.0;
    print($"${(Pi * r).toFixed(2)}");        // rustc: no method `toFixed` on f64
}
```

Status: FIXED in f5dacbf (a top-level constant read by name has its type).

**B42. An error thrown by a Rust call cannot be caught.** (Circle, Guess,
Quadratic, not failing only because their input is valid) A Rust `Result`
becomes `throws` (Bindgen §G.5.4) and the `Err` is raised with
`panic_any(err)`, but every `catch` downcasts to a Jux exception class, so
`catch (Exception e)` and `catch (Error e)` both miss it. The program exits
with 101 and prints nothing, because the panic hook only prints string
payloads.

```jux
void main() {
    String text = "abc";
    try {
        double d = text.parse<double>();
        print(d);
    } catch (Exception e) {
        print("not a number");                  // never reached; exit 101, no message
    }
}
```

Status: FIXED in 1c3483c (a Rust error is an exception; Bindgen G.5.4).

**B43. `rand_pcg` generators have no `SeedableRng`/`RngCore` methods.**
(Guess) The stub for `Lcg128Xsl64` (`Pcg64`) lists its constructor and
`advance` only; the trait impls that come from `rand_core` are not
discovered, so `seed_from_u64`, `next_u64` and `random_range` are all E0413
on it. Through the alias the call is not even checked:
`Pcg64.seed_from_u64(1uL)` passes juxc and leaks E0423 (`Pcg64.` on a Rust
type alias). `rand`'s own `StepRng` does get `Rng`, which is what Guess uses.

```jux
import rust.rand_pcg.Pcg64;
void main() {
    var rng = Pcg64.seed_from_u64(2026uL);     // rustc E0423
    print(rng.next_u64());
}
```

Status: FIXED in 36bc3b2.

**B44. `Duration` is not reachable.** (Launch) The `rust.std` stub declares
`sleep(Duration dur)` and a dozen methods taking a `Duration`, but the type
itself (`core::time::Duration`, re-exported as `std::time::Duration`) has
no declaration: `import rust.std.Duration;` is E0301. Launch uses the older
`sleep_ms(u32)`.

Status: FIXED in 96110d8.

**B45. A declaration that starts with `unsafe` does not parse.** (Pointer)
Layout-ABI §L.5 gives `unsafe-fn = 'unsafe' function-decl` and the example
`unsafe public void mmio_write(...)`, but at top level a leading `unsafe` is
read as an `unsafe { }` statement: E0200 "top-level statements have nowhere
to run", then E0301 at every call. `public unsafe void f(...)` works, and the
lesson uses it.

```jux
unsafe void bump(long* p) {       // E0200
    p[0] += 1;
}
void main() {
    long v = 1;
    unsafe { bump(&v); }           // E0301: cannot find `bump`
    print(v);
}
```

Status: FIXED in 9274da2.

**B46. A String passed to a native function is moved.** (Extern) The
marshalling emits `CString::new(text)`, which consumes the `String`, so any
later use of it, in the same call or after it, leaks E0382.

```jux
@extern(lib = "kernel32")
unsafe native {
    i32 lstrlenA(String s);
}
void main() {
    String text = "hello";
    unsafe {
        i32 n = lstrlenA(text);
        print($"${n} ${text}");       // rustc E0382: borrow of moved value `text`
    }
}
```

Status: FIXED in c6d09e2.

**B47. `sizeof` is typed `int`, not `uint`, and emitted as `usize`.**
(Union, Memory, Allocator) JUX-LANG-V1 §5.9.1 says `sizeof(T)` is a `uint`.
juxc types it as `int`, so `count * sizeof(ulong)` with a `ulong` count is
E0410 (a `uint` would widen), while the Rust it emits is a `usize`, so every
`int` context leaks E0308: `int s = sizeof(i32);`, `f(sizeof(long))` for an
`int` parameter, a `sizeof` among `int...` arguments. Printing works, and an
explicit `as` cast works everywhere, which is what the lessons use.

```jux
int one(int a) { return a; }
void main() {
    int s = sizeof(i32);            // rustc E0308: expected isize, found usize
    print(one(sizeof(long)));       // same
}
```

Status: FIXED in 9cb64a3.

**B48. Smaller issues.**
- A throwing Rust method called on a string LITERAL is not unwrapped:
  `double d = "abc".parse<double>();` is emitted as `let d: f64 =
  "abc".to_string().parse::<f64>();` and leaks E0308 (a `Result` where an
  `f64` is expected). The same call on a `String` local is unwrapped.
- A Rust method returning `Option<&str>` leaks: `String? s =
  temp_dir().join("x").to_str();` is emitted with `.cloned()` on the
  `Option<&str>` (E0599). `$"${path.display()}"` gives the text instead
  (Extern).
- Pointers guide §5.8 shows `var p = unsafe { malloc(bytes) };`, but
  Layout-ABI §L.5 allows `unsafe { }` only as a statement, and the compiler
  follows §L.5 (E0200). The guide's example should declare first and assign
  inside the block.
- `jux run --manifest-path` builds under `<project>/target` even with
  `CARGO_TARGET_DIR` set (B15 again), so each staged lesson has its own
  cargo target.

Status: the string-literal receiver FIXED in 1c3483c; `to_str()` on a `PathBuf` FIXED in 29be156; the pointers guide example rewritten to statement-form `unsafe { }` (208521a); `jux run` in project mode does build under `CARGO_TARGET_DIR` since 177cdcc, and only copies the finished binary into the project.
