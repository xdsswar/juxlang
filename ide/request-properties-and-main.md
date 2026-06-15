# Request: plugin awareness for implicit-nullable properties, flexible `main`, and the malformed-accessor diagnostic

> Handoff for the IntelliJ plugin maintainer. These are compiler/LSP changes
> (committed in the `juxc-*` crates). The plugin is **already structurally
> compatible** with all of them; this note lists what to verify and two optional
> enhancements. Diagnostics themselves flow from the LSP, so rebuilding the
> release toolchain (`cargo build --release`) is what surfaces them in the IDE.

## 1. Implicit-nullable auto-properties (new semantics)

An auto-property with **no initializer**, or with an explicit `= null`, is now
**implicitly nullable** and defaults to `null`. The compiler rewrites the property
type to `T?` during desugar:

```jux
public class Box<T> {
    public T Value { get; set; }              // implicitly T?, reads null until set
    public int Count { get; set; }            // implicitly int?, reads null until set
    public String Name { get; set; } = null;  // same: implicitly String?
}
```

The getter returns `T?`, the setter accepts `T?`, and a read before assignment is
`null`. A property with a real initializer (`= 0`, `= "x"`, ...) keeps its declared
non-nullable type. Spec: `JUX-MISSING-DEFS-ADDENDUM.md` §M.7.3.1.

**Verify in the plugin:**
- No inspection flags `T P { get; set; }` (no initializer) or `T P { get; set; } = null;`
  as an error. (Audited the current inspections: none do. `JuxPropertyNaming`,
  `JuxAccessorVisibility`, etc. are unaffected.)
- If the plugin does any property-read type inference / completion, the inferred type of
  an uninitialized auto-property is now **nullable** (`T?`), so reads should be treated as
  nullable (offer `!!` / null-check, not the bare `T` surface).

## 2. Flexible `main` entry point

`main` is accepted as a **free function** OR a **class `static` method**, in all three
signatures: `main()`, `main(String[] args)`, `main(String... args)`. Rules (already in
`JUX-ENTRY-POINTS-ADDENDUM.md` §E.1.2.2, now implemented):
- A class `main` **must be `static`** to be an entry point. A non-static class `main`
  shaped like an entry is just a method; it only errors (`E0326`) when there is no other
  valid entry in the file.
- A free `main` plus a class `static main` in the same file is ambiguous (`E0320`).

**Plugin status:** `JuxMainDetector` already matches `static`-modified class `main`, so the
Run gutter already appears for a class `static void main(...)`. No change needed. Just
confirm the Run line marker still shows for the three signatures.

## 3. Malformed accessor placement (new diagnostic)

Writing the accessor block **after** the `=` is a common mistake and now gets a precise
error instead of a cascade:

```jux
public T Second = { get; set; };   // WRONG: accessor block must precede `=`
// E0200: "the accessor block must come before `=` — write `Name { get; set; } = init;`"
```

The correct form is `Name { get; set; } = init;`. The LSP surfaces this E0200 directly.

**Optional enhancement:** a quick-fix that rewrites `T P = { get; set; };` to
`T P { get; set; };` (drop the `=`, move the block before any initializer) would be a nice
touch. Not required for correctness.

## 4. Distinct-until-changed observers (already implemented, informational)

A `{ get; set; }` property setter fires change-observers only when the new value differs
from the old (`if old != now`). This already worked and is unchanged; the only new wrinkle
is that an uninitialized property starts at `null`, so the first assignment is a real
`null -> value` transition that fires. Nothing to do in the plugin.

## 5. Build hygiene (driver, no plugin impact)

`jux build` now (a) skips rewriting emitted `.rs` whose content is unchanged (keeps cargo
incremental) and (b) prunes emitted `.rs` for `.jux` files that were removed/renamed. This
is entirely under `target/.rust-build/`; no plugin impact.

## Verification checklist
- [ ] `public T P { get; set; }` (no init) shows no plugin-side error; LSP reports none either.
- [ ] Run gutter appears for `class App { public static void main(String[] args) { } }`.
- [ ] `T P = { get; set; };` shows the E0200 hint (LSP) and ideally a quick-fix (optional).
- [ ] Property-read completion/inference treats an uninitialized auto-property as nullable.
