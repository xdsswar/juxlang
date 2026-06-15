# Request: resolve & color the implicit `value` symbol inside a property accessor body

> Handoff for the IntelliJ plugin maintainer. This is a **plugin-only** change —
> the compiler already handles it correctly (verified, see below). No `juxc-*`
> change is needed.

## Problem

Inside a custom property accessor body (the `set { ... }` block, and `get { ... }`
where applicable), the contextual keyword **`value`** — the implicit accessor
parameter holding the value being assigned (C#-style) — is flagged
`Cannot resolve symbol 'value'`. It should resolve cleanly and be **colored as a
variable** (same tier as a local/parameter), because it is exactly that: the
internal property value bound for the duration of the accessor body.

Reported example (a custom `set` that inspects `value`):

```jux
public String Title {
    get;
    set {
        if (value == null) {     // <-- 'value' here is unresolved in the IDE
            return;
        }
        // ... use value ...
    }
} = ""
```

## Expected behavior

- `value` resolves with **no** "Cannot resolve symbol" inspection anywhere inside
  a `set` accessor body. (Also inside a `get` body if the plugin lets one appear
  there; the canonical home is `set`.)
- `value` is highlighted as a **variable/parameter** (the `var`/parameter color),
  not as a keyword and not as an unresolved/error token.
- Its inferred type is the **property's effective type**: the declared property
  type, or its implicitly-nullable form (`T?`) for an uninitialized auto-property
  (see `ide/request-properties-and-main.md` §1). So completion / type hints on
  `value.` offer that type's surface.
- `value` is **only** in scope inside an accessor body. Outside accessors it is an
  ordinary identifier and must NOT be specially resolved.

## Implementation notes (suggested, plugin-side)

- Treat `value` as a **synthetic implicit parameter** of the accessor: when the
  PSI resolver is asked to resolve a reference named `value` whose enclosing
  context is an accessor body, return a synthetic light variable element whose
  type is the property's effective type (`effectiveTypeText` / `isImplicitlyNullable`
  helpers already exist on `JuxPsiElements`). This mirrors how Kotlin resolves the
  implicit `field`/`value` and how the plugin already handles other contextual
  keywords.
- For coloring: the annotator/highlighter should classify a `value` reference
  inside an accessor body as the variable text-attribute key (not keyword).
- The "Cannot resolve symbol" inspection (the live unresolved-ref census) must
  whitelist `value` in accessor scope — same mechanism used to avoid false
  positives for other in-scope synthetic bindings.

## Grounding (compiler side is already correct)

This program compiles and runs (`juxc Widget.jux --run`), printing `[Hi]` then
`[(empty)]` — i.e. the compiler resolves `value` in the setter and routes the
write through it:

```jux
class Widget {
    private String _t = "";
    public String Title {
        get { return _t; }
        set { if (value == "") { _t = "(empty)"; } else { _t = value; } }
    }
}
public void main() {
    var w = new Widget();
    w.Title = "Hi";  print($"[${w.Title}]\n");
    w.Title = "";    print($"[${w.Title}]\n");
}
```

So the only gap is editor-side resolution/coloring of `value`.
