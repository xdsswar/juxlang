# Rux lessons, in Jux

Rux ships 67 short teaching programs, one concept each: Hello, Variable,
Const, and so on up to allocators and inline assembly. This folder rewrites
them in Jux and runs them. They serve two purposes at once:

- **Teaching material.** Each `src/Main.jux` is commented the way the Rux
  original is, adapted to Jux: what the construct is, why it exists, and the
  trap worth knowing before relying on it.
- **A test corpus.** Each lesson has an `expected.txt` with its exact output,
  derived by reading the Rux program, and `bin/jux/tests/rux_lessons.rs`
  builds, runs and compares every one of them.

## Layout

```
tests/rux-lessons/
  README.md              this file: mapping rules and the results table
  known-failures.txt     lessons that fail today, one `Name: reason` per line
  <Name>/jux.toml        the project manifest (the only manifest; no module.jux)
  <Name>/src/Main.jux    the lesson (Module has more files, in packages)
  <Name>/expected.txt    exact stdout
  <Name>/stdin.txt       optional scripted input, piped to the program
```

## Running

```
cargo test -p jux --test rux_lessons                   # every lesson
RUX_LESSON=Hello,Range cargo test -p jux --test rux_lessons   # just these
```

The test discovers lesson folders from this directory, copies each one to
`target/rux-lessons/<Name>` so the build never writes into the sources, runs
`jux run --manifest-path` on the copy, and compares stdout with
`expected.txt`. Both sides are compared after unifying line endings, dropping
trailing whitespace on every line, removing `jux:` status lines, and trimming
leading and trailing blank lines. Everything else must match exactly.

All failures are reported together in one message. A lesson listed in
`known-failures.txt` may fail; a listed lesson that starts passing fails the
test with "remove it from known-failures", so a fixed bug cannot hide behind a
stale entry.

## Mapping rules

How a Rux construct becomes Jux, applied the same way in every lesson.

- `func Main() -> int` becomes `void main()` (or `int main()` where the exit
  code matters); `PrintLine("x {}", v)` becomes `print($"x ${v}")`.
- `let` / `var` become `final` locals / non-final locals (`var` or a written
  type); `match` becomes `switch` with `case ... ->` arms; `Option<T>` becomes
  `T?`; `Result` and `?` use the core `Result<T, E>` (`Result.Ok`,
  `Result.Err`, `expr?`) per Core lib §K.4, with exceptions mentioned where
  they are the better Jux answer.
- Rux collections become the Rust std ones under their Rust names (`Vec`,
  `VecDeque`, `HashMap`, `HashSet`, `BTreeMap`) and Rust method names (`push`,
  `len`, `contains`, `sort_unstable`), with no allocator argument: the full
  profile uses the global heap.
- Equality, ordering, hashing and text are operator overrides
  (`operator==`, `operator<=>`, `operator hash`, `operator string`), never
  methods. `Core::Equatable` maps to `operator==` (plus `operator hash`, which
  Jux requires alongside it).
- `variant` becomes an `enum` with payloads; `extend Type` blocks become
  methods inside the type; `extend Type : Interface` becomes
  `implements Interface`; `struct` stays `struct` (a value type in both).
- A Rux function taking `T[..]` takes a Jux array `T[]` (any length) or a
  `Vec<T>`; Jux arrays and `Vec` are reference types.
- `Variadic` uses `T... name` (sugar for `T[]`); a Rux `Display...` list of
  mixed types becomes `any...`.
- `Module` becomes a multi-package project: one `jux.toml`, the entry point in
  `src/Main.jux`, and each Rux `module` a Jux `package` in a directory that
  mirrors its name.
- Later waves: Json uses `rust.serde_json`; Random and Password use
  `rust.rand` + `rust.rand_pcg` with a fixed seed; Age and Time use
  `rust.chrono`; Unicode uses `rust.unicode-segmentation`; File, Directory,
  Path and Binary use `rust.std`; `defer` becomes `try`/`finally` or a `drop`
  block; `Alloc`/`Free`/`Memory`/`Pointer` use unsafe pointers with
  `new`/`delete`; `when`, `#target` and `#build` use `@cfg`. Real gaps
  (`union`, inline `asm`, arena and custom allocators, compiler-version
  queries) are written as far as Jux allows and marked GAP.

## Where Jux prints differently by design

`expected.txt` follows Jux semantics, not Rux output, wherever the two
languages deliberately differ. The differences met so far:

- **Floating point text.** A `double` prints like Java's `Double.toString`
  and a `float` like `Float.toString`: `7.0` rather than `7`, `3.0 x 4.0`
  rather than `3 x 4`, and `3.1415927` for a `float` pi. (Convert, Struct,
  Tuple, Variant, Interface, TypeAlias, Math, Statistics.)
- **Ties round to even.** `round()` and `toFixed(n)` break an exact tie
  towards the even digit: `(-2.5).round()` is `-2.0` and `0.125.toFixed(2)`
  is `0.12`. Rux, like Rust's `round`, sends ties away from zero. (Math,
  Format.)
- **No placeholder specs.** Jux interpolation has no `{:>8}` or `{:08b}`.
  Bases come from `toBinary()`, `toOctal()`, `toHex()`; precision from
  `toFixed(n)`; width, fill and alignment from a few lines of helper code in
  the lesson. The rendered text is the same. (Primitive, Operator, Convert,
  Format, Console, Variant, Propagate, Overloading, Algorithm.)
- **One `print`.** `print` always ends the line; Rux's `Print` (no newline)
  pieces are assembled into a `String` and printed whole. The output is the
  same except that trailing spaces are not significant. (Loop, Range,
  Console, Thanks, Prime, Statistics, Algorithm.)
- **One `bool` and one `char`.** Rux has 8/16/32-bit booleans and
  characters; Jux has one of each, with C widths spelled at the FFI boundary.
  The Primitive lesson keeps the labels and prints the same values.
- **Every `switch` is exhaustive.** A Rux `match` on a number that fits no
  arm does nothing; in Jux that is error E0440, and silence needs an explicit
  empty `default`. (Match.)
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
- **Names.** Lesson text that names the language itself (Console, Format,
  Thanks) says "Jux".

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
| Ternary | 1 | PASS | the Rux "bind text operands first" caveat does not apply |
| Loop | 1 | PASS | `loop` becomes `while (true)` |
| Range | 1 | PASS | overflow panics in debug instead of wrapping |
| Function | 1 | PASS | |
| Overload | 1 | PASS | |
| Callback | 1 | FAIL | B1: a free function passed as a value leaks rustc E0308 |
| Generic | 1 | PASS | `where T has operator<=>(T) -> int` bound |
| Variadic | 1 | PASS | `any...` for mixed types, `int...` accepts an array |
| Array | 1 | PASS | |
| Slice | 1 | FAIL | B3, B4, B5 (record static call, user `length()`, array component); no slice syntax, view built by hand |
| Tuple | 1 | PASS | |
| Struct | 1 | PASS | |
| Enum | 1 | PASS | C-layout enum for discriminants, `ordinal()` for plain |
| Variant | 1 | PASS | enum payloads; printing derives `Range(low: 20.0, high: 24.0)` |
| Match | 1 | PASS | exhaustive switch; `case 6, 7 ->` added |
| Option | 1 | PASS | `int?`, null narrowing, `?:` |
| Result | 1 | FAIL | B2: switching on a `Result` leaks rustc E0308 |
| Propagate | 1 | FAIL | B2 again (the `?` path itself is fine) |
| TypeAlias | 1 | PASS | `{...}` needs `new T[]` under an array alias (B13) |
| Interface | 1 | PASS | `operator==` needs `operator hash` |
| Iterator | 1 | PASS | `Iterator<T>`/`Iterable<T>` classes |
| Overloading | 1 | PASS | `operator<=>` instead of a lone `<` |
| Ownership | 1 | PASS | `move` not implemented; shows sharing plus `drop` |
| String | 1 | PASS | `substringBytes` missing (B11), boundary test done on bytes |
| Format | 1 | PASS | helpers instead of specs; `0.125` ties to `0.12` |
| Math | 1 | GAP | B14: Rust `f64` methods (`powf`, `ln`, `sin`, ...) not reachable on `double` |
| Prime | 1 | PASS | `Vec<bool>` |
| Statistics | 1 | FAIL | B8: runtime panic, `for (i : 1..v.len())` holds the Vec borrowed |
| Algorithm | 1 | FAIL | B6, B7; iterator adaptors and `sort` missing (B14), written as helpers |
| Console | 1 | PASS | |
| Thanks | 1 | PASS | |
| Module | 1 | PASS | three packages; package-private leak found (B9) |
| Vector | 2 | TODO | |
| Deque | 2 | TODO | |
| HashMap | 2 | TODO | |
| TreeMap | 2 | TODO | |
| Json | 2 | TODO | |
| Random | 2 | TODO | |
| Password | 2 | TODO | |
| Unicode | 2 | TODO | |
| Age | 2 | TODO | |
| Time | 2 | TODO | |
| File | 2 | TODO | |
| Directory | 2 | TODO | |
| Path | 2 | TODO | |
| Binary | 2 | TODO | |
| Circle | 3 | TODO | |
| Guess | 3 | TODO | |
| Launch | 3 | TODO | |
| Quadratic | 3 | TODO | |
| Melody | 3 | TODO | |
| Defer | 3 | TODO | |
| Memory | 3 | TODO | |
| Pointer | 3 | TODO | |
| Config | 3 | TODO | |
| Version | 3 | TODO | |
| Extern | 3 | TODO | |
| Union | 3 | TODO | |
| Asm | 3 | TODO | |
| Allocator | 3 | TODO | |

Wave 1 totals: 32 PASS, 6 FAIL, 1 GAP.

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

**B3. A static method call on a record leaks E0423.** (Slice)

```jux
record P(int x) {
    static P zero() { return new P(0); }
}
void main() { print(P.zero().x); }   // emitted as `P.zero()`, not `P::zero()`
```

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

**B5. Indexing an array component inside a record leaks E0608.** (Slice)

```jux
record A(int[] items) {
    int first() { return items[0]; }   // self.items[0] on Rc<JuxCell<Vec<isize>>>
}
void main() { final int[] xs = {4, 5}; print(new A(xs).first()); }
```

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

**B9. A package-private free function is visible from another package.**
(Module) Language §4.4 makes a declaration with no modifier visible within its
package only, and reaching it from elsewhere E0416. In the Module project,
`src/Main.jux` can `import rux.module.geometry.doubled;` and call
`doubled(5)`, though `doubled` has no modifier; it compiles and prints `10`.
The lesson follows the spec and does not rely on this.

**B10. `Result.err()` is missing.** Core lib §K.4 lists `Option<E> err()`;
`r.err()` is E0413 "no method `err` on type `jux.std.result.Result`".

**B11. `String.substringBytes` is missing.** Core lib §K.7 lists
`substringBytes(int, int) throws EncodingException, IndexOutOfBoundsException`;
calling it is E0413. The String lesson tests the boundary on `bytes()`
instead.

**B12. An error inside `${...}` on a parenthesized receiver is reported at
1:1.** `print($"${(2.0).nosuch()}");` gives
`file.jux:1:1: [E0413] no method nosuch on double`; the same call without
the parentheses reports the right line and column.

**B13. `{...}` under an array type alias is a parse error.**
`type Bytes = ubyte[]; final Bytes data = {1, 2, 3};` gives three E0200
"expected expression" errors and a follow-on E0601. `new ubyte[] {1, 2, 3}`
works. Either the initializer should be accepted or the message should say
that a `{...}` initializer needs the array type written out.

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
