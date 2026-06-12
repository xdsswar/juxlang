# Jux Spec Addendum — Observable Properties

**Status:** Normative. Targets JUX-LANG-V1.md §5 (Type System) and §7 (Class Declarations).
Builds on `JUX-MISSING-DEFS-ADDENDUM.md` §M.7 (property accessor syntax) and edits
`JUX-GRAMMAR-ADDENDUM.md` §A.1.3 / §A.2.5 / §A.2.7.
**Sigil:** §P

This addendum specifies Jux's observable property system: the C#-style `{ get; set; }`
declaration syntax, the PascalCase naming convention, the `observer<T>` primitive type,
the `.observers` built-in member, property binding primitives, computed properties, and
the IntelliJ/jux-ls diagnostics that surface all of the above.

§M.7 remains the base spec for accessor syntax (auto-properties, custom bodies,
asymmetric visibility, static properties, mutation inference). This addendum layers
observability and binding on top of it, and **removes the `init` accessor** — the
accessor kinds are `get` and `set` only. Read-only construction-time properties are
expressed as `{ get; }` (settable in the constructor, per §M.7.2).

---

## Design Philosophy (Non-Normative)

Jux properties are designed around three principles:

1. **Zero ceremony for the common case.** A property declaration looks like a field
   declaration. No wrapper type, no base class, no interface to implement.

2. **Observable by default.** Every `{ get; set; }` property is observable. The
   infrastructure is lazily initialized — no overhead until an observer is attached.

3. **Weak by default.** All observers are held as weak references. If the observer's
   owner is dropped, the observer silently stops firing and is removed. No leaks, no
   dangling callbacks, no manual cleanup required.

These three principles together eliminate the verbose boilerplate that makes JavaFX
properties painful and the lifecycle bugs that make event-driven code fragile.

---

## §P.1 — Property Declaration Syntax

### P.1.1. Basic Form

A property is declared using C#-style accessor syntax. The `{ get; set; }` block
distinguishes a property from a plain field. **Property names are preferably
PascalCase** — starting with an uppercase letter — as the visual signal that a name
is a property and not a plain field. This is a convention, **not a requirement**:
a camelCase property such as `private String test { get; set; } = "test";` is
perfectly legal.

```java
public int Size { get; set; } = 0;
public String Name { get; set; } = "";
public Color Fill { get; set; } = Color.Black;
public bool Visible { get; set; } = true;
public Connection Conn { get; set; } = null;
public List<User> Users { get; set; } = null;
```

Plain fields conventionally stay camelCase — no `{ get; set; }`, no observers,
no binding:

```java
// Plain fields — camelCase, no property infrastructure
private int count;
private String name;
private bool active;
```

The PascalCase convention is surfaced as a suppressible warning (`W0974`) by the
compiler and as a rename hint by the IDE. It never blocks compilation.

The type is any valid Jux type. There is no wrapper type — `int` is `int`, `String` is
`String`. The `{ get; set; }` syntax is the property declaration, not a type.

### P.1.2. Uninitialized Properties

A property declared without an initializer defaults to `null` (or the zero value for
primitives):

```java
public String Now { get; set; };    // null by default
public int Count { get; set; };     // 0 by default
```

### P.1.3. Visibility on Accessors

The getter and setter may carry independent visibility modifiers. The setter may not be
more visible than the getter — the compiler enforces this statically.

```java
// Public get, public set (default — both inherit property visibility)
public String Name { get; set; } = "";

// Public get, private set — read outside, only class can write
public String Id { get; private set; } = "";

// Public get, protected set — read outside, subclasses can write
public int Count { get; protected set; } = 0;

// Private both
private String Secret { get; set; } = "";

// Illegal — setter more visible than getter
public String Bad { private get; set; } = "";  // E0972: setter visibility
                                                // exceeds getter visibility
```

Supported visibility levels on accessors: `public`, `protected`, `private`. There is no
package-level visibility on properties.

### P.1.4. Custom Accessor Bodies

Accessor bodies may be block-form or expression-form. Inside a setter body, `value`
refers to the incoming value (C# convention).

**Expression-form getter:**

```java
private int _age;
public int Age {
    get -> _age;
    set { if (value > 0) { _age = value; } }
};
```

**Block-form both:**

```java
private String _name = "";
public String Name {
    get { return _name.trim(); }
    set { _name = value != null ? value : ""; }
};
```

**Expression-form both:**

```java
public double Celsius { get; set; } = 0.0;
public double Fahrenheit {
    get -> Celsius * 9.0 / 5.0 + 32.0;
    set { Celsius = (value - 32.0) * 5.0 / 9.0; }
};
```

When a custom setter body is provided, the compiler still fires observers after the
body executes — the user does not call `fireObservers()` manually.

### P.1.5. Read-Only Computed Properties

A property with only a getter is a computed property. Its value is derived from other
properties; the compiler tracks dependencies automatically and recomputes when any
dependency changes.

```java
public String First { get; set; } = "";
public String Last { get; set; } = "";

// Computed — recomputes when First or Last changes
public String FullName { get -> $"${First} ${Last}"; };

// More complex
public bool IsEmpty { get -> Users.size == 0; };
public double Area { get -> Width * Height; };
public String Label { get -> $"${Name} (${Age})"; };
```

Computed properties:
- Have no setter. Assigning to them is `E0970`.
- Fire their observers when the computed value changes.
- Are lazily evaluated — recomputed only when accessed after a dependency change.
- May be observed via `.observers` exactly like settable properties.

---

## §P.2 — The `observer<T>` Primitive

### P.2.1. Declaration

`observer<T>` is a **reserved primitive keyword**, colored as a primitive type in the
IDE alongside `int`, `bool`, `void`. It is the type of a callable that observes a
property.

```java
private final observer<String> obs = (old, now) -> {
    print($"changed: $old → $now");
};

private final observer<int> sizeWatcher = (old, now) -> {
    updateLayout();
};
```

### P.2.2. Lambda Shapes

`observer<T>` accepts three lambda shapes. The shape determines the behavior when the
observer is attached to a property.

**Shape 1 — Full observer.** Fires when the property value actually changes. Receives
the old and new values.

```java
private final observer<String> obs = (old, now) -> {
    if (now != null) {
        print(now);
    }
};
```

**Shape 2 — Full observer with property reference.** Same as Shape 1 but also receives
a reference to the property that fired. Useful when a single observer is attached to
multiple properties.

```java
private final observer<String> obs = (prop, old, now) -> {
    print($"property fired: $now");
    // prop is a reference to the firing property
};
```

**Shape 3 — Invalidation observer.** Fires when the property is invalidated (marked
dirty), regardless of whether the value has changed. Receives no values.

```java
private final observer<String> inv = () -> {
    markLayoutDirty();
};
```

### P.2.3. All Observers Are Weak by Default

Properties hold **weak references** to their observers. This is the only mode — there
is no strong-reference observer variant and no `weak` keyword modifier needed.

Consequences:
- If the observer's owner is dropped, the observer silently stops firing.
- Dead observers are detected and removed on the next firing of the property.
- No manual `detach()` is required when a component is destroyed.
- No retain cycles can form through the observer relationship.

```java
public class MyController {
    private final observer<String> nameObs = (old, now) -> {
        Label.Text = now;
    };

    public MyController(Model model) {
        model.Name.observers.attach(nameObs);
        // No manual cleanup needed — when MyController is dropped,
        // nameObs is dropped with it, the weak ref goes null,
        // and model.Name silently removes it on next fire.
    }
}
```

### P.2.4. Named Observer Variables

Observer variables may be declared at any scope — field, local, parameter. They are
reusable across multiple properties.

```java
// Field-level — shared across methods
private final observer<int> countWatcher = (old, now) -> {
    updateBadge(now);
};

// Local — used once
public void setup() {
    var once = (old, now) -> {
        print("first change");
    };
    Name.observers.attach(once);
}

// Typed explicitly when needed
private observer<String> o;  // declared, not yet initialized
```

---

## §P.3 — The `.observers` Member

### P.3.1. Overview

Every `{ get; set; }` property automatically exposes an `.observers` member. This
member is **not reserved** but is **native-colored** by the IDE when accessed on a
property. It is the namespace through which all observer operations are performed.

### P.3.2. Operations

**`.attach(observer)`** — Registers an observer. The property holds a weak reference.
The lambda shape determines full or invalidation behavior.

```java
Name.observers.attach(obs);        // full or invalidation — determined by shape
Name.observers.attach(inv);
Name.observers.attach((old, now) -> { print(now); });  // inline
Name.observers.attach(() -> { markDirty(); });          // inline invalidation
```

**`.detach(observer)`** — Removes a specific observer.

```java
Name.observers.detach(obs);
```

**`.clear`** — Removes all observers and releases the underlying storage. Note: no
parentheses — `clear` is a property-like command accessor.

```java
Name.observers.clear;
```

**`.size`** — Returns the number of currently live observers. Note: no parentheses.

```java
int count = Name.observers.size;
if (Name.observers.size == 0) { ... }
```

### P.3.3. Lazy Initialization

Observer storage is not allocated until the first `.observers.attach(...)` call. A
property with no attached observers allocates nothing beyond its value field. The setter
performs one null check — effectively free.

`.observers.size` returns `0` before any attach without triggering allocation.

### P.3.4. Firing Order

When a property value changes, the setter fires in this order:

1. **Invalidation observers** (Shape 3) — fire first, before the value is considered
   fully committed. For layout systems that want to mark dirty before rendering.
2. **Full observers** (Shape 1 and 2) — fire after, with old and new values confirmed.

Dead weak references are detected and pruned during each firing pass using `retain()`.

### P.3.5. Observer Access Follows Getter Visibility

`.observers.attach()` is accessible wherever the getter is accessible. If the getter is
`private`, external code cannot attach observers. If the getter is `public`, any code
can attach observers.

```java
public String Status { get; private set; } = "active";

// Outside the class:
node.Status                          // ✅ readable
node.Status = "inactive";           // ❌ E0972: setter is private
node.Status.observers.attach(obs);  // ✅ observable — getter is public
```

---

## §P.4 — Property Binding

### P.4.1. Overview

Every `{ get; set; }` property supports binding operations. These are **native-colored**
when used after a property and are **not reserved keywords**. Binding is built on the
observer infrastructure internally.

### P.4.2. One-Way Binding

`bind(source)` — Establishes a one-way binding. The target follows the source. The
target is set immediately to the source's current value, then updated on every
subsequent change.

```java
// NameLabel.Name follows NameField.Name
NameLabel.Name.bind(NameField.Name);
```

Setting a bound property directly is `E0973` at compile time (when detectable) and
throws `IllegalStateException` at runtime in debug builds.

### P.4.3. Bidirectional Binding

`bindBidirectional(other)` — Both properties follow each other. An internal `updating`
guard prevents infinite recursion.

```java
Slider.Value.bindBidirectional(ProgressBar.Progress);
```

Both properties must be the same type. The compiler enforces this statically (`E0974`).
Bidirectional binding requires setter access on both properties at the call site.

### P.4.4. Unbinding

`unbind()` — Breaks any active binding (one-way or bidirectional). Safe to call when
not bound.

```java
NameLabel.Name.unbind();
Slider.Value.unbind();
```

A single `unbind()` handles both directions — no need to know which kind of binding was
established.

### P.4.5. Binding and Observers

Bindings are implemented as observers internally. Observers attached before or after
binding is established fire normally when the property value changes — whether driven by
a binding or by a direct set.

---

## §P.5 — Coloring and Reserved Status

| Token | Context | IDE Color | Reserved |
|---|---|---|---|
| `observer` | Anywhere | Primitive keyword color | ✅ Yes |
| `observers` | After `{ get; set; }` property | Native member color | ❌ No |
| `attach` | After `.observers` | Native operation color | ❌ No |
| `detach` | After `.observers` | Native operation color | ❌ No |
| `clear` | After `.observers` | Native operation color | ❌ No |
| `size` | After `.observers` | Native operation color | ❌ No |
| `bind` | After `{ get; set; }` property | Native operation color | ❌ No |
| `unbind` | After `{ get; set; }` property | Native operation color | ❌ No |
| `bindBidirectional` | After `{ get; set; }` property | Native operation color | ❌ No |

Tokens marked ❌ in the Reserved column receive native coloring **only in property
context**. Used elsewhere they are plain identifiers with no special treatment:

```java
var bind = "hello";               // plain identifier — no special color
myObject.attach(x);               // plain method call — no special color
Name.observers.attach(obs);       // native colored — property context
Name.bind(other.Name);            // native colored — property context
```

---

## §P.6 — Diagnostics

The following diagnostic codes are emitted by `juxc` and surfaced as warnings or errors
in the IntelliJ plugin, jux-ls (LSP server), and the VS Code extension.

Property diagnostics live in the `E097x` / `W097x` family of the master catalog
(`JUX-DIAGNOSTICS-ADDENDUM.md` §D.4). `E0970` and `E0972` predate this addendum
(introduced by §M.7) and are reused here — same meaning, broader coverage.

| Code | Severity | Condition |
|---|---|---|
| `E0970` | Error | Assignment to a read-only or computed (get-only) property (pre-existing, §M.7.2) |
| `E0972` | Error | Property accessor visibility violation — setter declared more visible than getter, or write through an inaccessible setter (pre-existing, §M.7.7) |
| `E0973` | Error | Direct assignment to a bound property |
| `E0974` | Error | `bindBidirectional` called with mismatched property types |
| `E0975` | Error | `observer<T>` lambda shape does not match any accepted form |
| `W0970` | Warning | `observer<T>` attached but never detached and target has no `drop` |
| `W0971` | Warning | Property declared with `{ get; set; }` but never observed or bound |
| `W0972` | Warning | Binding established but source property has `private set` — binding will never update |
| `W0973` | Warning | Custom setter body contains an early `return` that may skip the observer fire |
| `W0974` | Warning | Property name does not start with an uppercase letter (PascalCase convention — preferred, never enforced) |

---

## §P.7 — IntelliJ Plugin and jux-ls Warnings

The IntelliJ plugin (`jux-intellij`) and the language server (`jux-ls`) surface all
diagnostics above as inline annotations. In addition, the IDE provides the following
property-specific inspections beyond what `juxc` emits at compile time.

### P.7.1. PascalCase Convention Hint (W0974)

The IDE suggests — as a hint, not an error — renaming property names that do not start
with an uppercase letter, with a quick-fix that renames the property and all its usages:

```
⚠ Property 'name' should be PascalCase
  → Quick fix: Rename to 'Name'
```

The quick-fix uses the IntelliJ rename refactoring — it updates the declaration, all
reads, all writes, all `.observers.attach(...)` call sites, and all `.bind(...)` call
sites in one operation.

### P.7.2. Unused Property Warning (W0971)

The IDE warns when a property is declared with `{ get; set; }` but no observer is ever
attached and no binding is ever established. This suggests the developer may have
intended a plain field instead:

```
💡 Property 'Count' is never observed or bound
   Consider using a plain field instead, or attach an observer
```

Suppressed if the property is `public` — it may be observed by external code the IDE
cannot see.

### P.7.3. Observer Naming Convention

The IDE suggests naming observer variables with a suffix that reflects the property they
observe. Not enforced — informational only:

```
💡 Consider naming this observer 'NameObs' or 'OnNameChanged'
   to clarify which property it observes
```

### P.7.4. Binding Type Mismatch (E0974)

The IDE highlights mismatched types in `bind()` and `bindBidirectional()` calls inline,
before compilation:

```java
Label.Name.bindBidirectional(Spinner.Value);
//         ^^^^^^^^^^^^^^^^^^               ❌ E0974
//         String cannot bind to double
```

### P.7.5. Bound Property Write (E0973)

The IDE highlights any direct assignment to a currently bound property:

```java
Label.Name.bind(Field.Name);
Label.Name = "hello";         // ❌ E0973 — Name is bound, cannot assign directly
```

### P.7.6. Custom Setter Without Observer Fire

When a custom setter body is provided, the IDE warns if the body contains a `return`
statement before the end — observers would not fire for early returns:

```java
public String Name {
    get -> _name;
    set {
        if (value == null) return;   // ⚠ W0973: early return may skip observer fire
        _name = value;
    }
};
```

The compiler fires observers after the setter body completes, but an early `return`
exits the body early and observers fire with whatever value was set at that point.

### P.7.7. Native Coloring in the Editor

The IDE applies distinct coloring to all property-context tokens as specified in §P.5:

- `observer` — same color as `int`, `bool`, `void` (primitive keyword)
- `observers` — same color as `.length` on arrays (built-in member)
- `attach`, `detach`, `clear`, `size` — same color as built-in operations, only
  when appearing after `.observers`
- `bind`, `unbind`, `bindBidirectional` — same color as built-in operations, only
  when appearing directly after a `{ get; set; }` property

This coloring is context-sensitive — the same token used elsewhere receives no special
treatment. The IDE's semantic highlighting pass (not lexer-based) applies these colors
after type resolution.

### P.7.8. Gutter Icons

The IntelliJ plugin adds a gutter icon next to every `{ get; set; }` property
declaration showing:

- 🔵 if the property has observers attached somewhere in the project
- 🔗 if the property is bound or is a binding source somewhere in the project
- ⬜ if neither (unobserved, unbound)

Clicking the icon navigates to all `.observers.attach(...)` call sites or `.bind(...)`
call sites for that property, using IntelliJ's existing "Find Usages" infrastructure.

---

## §P.8 — Rust Lowering

### P.8.1. Property Fields

For each `{ get; set; }` property, the compiler emits a backing field plus two optional
observer vecs (lazily initialized):

```rust
// For: public String Name { get; set; } = ""
_Name: String,
_Name_full: Option<Vec<Weak<dyn Fn(&str, &str)>>>,   // Shape 1 + 2
_Name_inv:  Option<Vec<Weak<dyn Fn()>>>,              // Shape 3
```

### P.8.2. Generated Setter

```rust
pub fn set_Name(self_: &Rc<RefCell<Self>>, new_value: String) {
    let old = self_.borrow()._Name.clone();
    if old != new_value {
        self_.borrow_mut()._Name = new_value.clone();

        // Invalidation observers fire first — prune dead weak refs
        if self_.borrow()._Name_inv.is_some() {
            self_.borrow_mut()
                ._Name_inv.as_mut().unwrap()
                .retain(|weak| {
                    if let Some(f) = weak.upgrade() {
                        f();
                        true
                    } else {
                        false  // dead — prune
                    }
                });
        }

        // Full observers fire second — prune dead weak refs
        if self_.borrow()._Name_full.is_some() {
            let old2 = old.clone();
            let now2 = new_value.clone();
            self_.borrow_mut()
                ._Name_full.as_mut().unwrap()
                .retain(|weak| {
                    if let Some(f) = weak.upgrade() {
                        f(&old2, &now2);
                        true
                    } else {
                        false  // dead — prune
                    }
                });
        }
    }
}
```

### P.8.3. Generated Observer Operations

```rust
// attach — routes by arity, stores as Weak
pub fn Name_observers_attach_full(
    self_: &Rc<RefCell<Self>>,
    f: Rc<dyn Fn(&str, &str)>
) {
    let mut s = self_.borrow_mut();
    if s._Name_full.is_none() {
        s._Name_full = Some(Vec::new()); // lazy init
    }
    s._Name_full.as_mut().unwrap().push(Rc::downgrade(&f));
}

pub fn Name_observers_attach_inv(
    self_: &Rc<RefCell<Self>>,
    f: Rc<dyn Fn()>
) {
    let mut s = self_.borrow_mut();
    if s._Name_inv.is_none() {
        s._Name_inv = Some(Vec::new()); // lazy init
    }
    s._Name_inv.as_mut().unwrap().push(Rc::downgrade(&f));
}

// detach
pub fn Name_observers_detach(
    self_: &Rc<RefCell<Self>>,
    target: &Rc<dyn Fn(&str, &str)>
) {
    if let Some(obs) = &mut self_.borrow_mut()._Name_full {
        obs.retain(|weak| {
            weak.upgrade().map_or(false, |f| !Rc::ptr_eq(&f, target))
        });
    }
}

// clear — releases allocation entirely
pub fn Name_observers_clear(self_: &Rc<RefCell<Self>>) {
    let mut s = self_.borrow_mut();
    s._Name_full = None;
    s._Name_inv  = None;
}

// size — no allocation triggered
pub fn Name_observers_size(self_: &Rc<RefCell<Self>>) -> usize {
    let s = self_.borrow();
    let full = s._Name_full.as_ref().map(|v| v.len()).unwrap_or(0);
    let inv  = s._Name_inv.as_ref().map(|v| v.len()).unwrap_or(0);
    full + inv
}
```

---

## §P.9 — Complete Example

```java
package com.example.app;

public class LoginForm extends VBox {
    // Properties — PascalCase
    public String Username { get; set; } = "";
    public String Password { get; set; } = "";
    public bool   Loading  { get; set; } = false;
    public String Error    { get; set; } = "";

    // Computed property — PascalCase, get only
    public bool CanSubmit {
        get -> Username.length > 0
            && Password.length >= 8
            && !Loading;
    };

    // Named observers
    private final observer<bool> loadingObs = (old, now) -> {
        Spinner.Visible = now;
        SubmitButton.Disabled = now;
    };

    private final observer<String> errorObs = (old, now) -> {
        ErrorLabel.Visible = now != null && now.length > 0;
        ErrorLabel.Text    = now ?? "";
    };

    // Invalidation observer
    private final observer<String> layoutInv = () -> {
        requestLayout();
    };

    // Observer with property reference
    private final observer<String> logObs = (prop, old, now) -> {
        print($"property changed: $old → $now");
    };

    public LoginForm() {
        // Attach observers
        Loading.observers.attach(loadingObs);
        Error.observers.attach(errorObs);
        Username.observers.attach(layoutInv);
        Password.observers.attach(layoutInv);
        Username.observers.attach(logObs);

        // Bind computed to button
        SubmitButton.Disabled.bind(CanSubmit);

        // Check count
        print($"observers on Loading: ${Loading.observers.size}");
    }

    public async void submit() throws AuthError {
        Loading = true;
        Error = "";

        try {
            await authService.login(Username, Password);
        } catch (AuthError e) {
            Loading = false;
            Error = e.message;
        }
    }
}
```

---

## §P.10 — Integration Notes

- This addendum targets **§5 (Type System)** for `observer<T>` and **§7 (Class
  Declarations)** for property declaration syntax. The base accessor syntax remains
  `JUX-MISSING-DEFS-ADDENDUM.md` **§M.7**; this addendum layers observability and
  binding on top of it.
- The **`init` accessor is removed** from §M.7 and from the grammar
  (`JUX-GRAMMAR-ADDENDUM.md` §A.2.5: `accessor-kind = 'get' | 'set'`). The `init`
  keyword itself stays reserved for `init { ... }` blocks (§M.1).
- `observer` joins the reserved keyword table in **§3.2** (and grammar §A.1.3);
  `observer '<' type '>'` joins the type grammar (§A.2.7).
- Property diagnostics live in the **`E097x` / `W097x`** family of
  `JUX-DIAGNOSTICS-ADDENDUM.md` §D.4: `E0970` / `E0972` (pre-existing, §M.7) plus
  the new `E0973`–`E0975` and `W0970`–`W0974`.
- PascalCase property naming is a **convention, not a rule** — surfaced as `W0974`
  and an IDE rename hint; camelCase properties compile without complaint beyond the
  suppressible warning.
- Property accessor visibility rules extend the visibility rules in **§7.2**.
- The borrow inference pass in **§6** treats property setters as mutation sites — the
  same ownership rules apply to property writes as to method calls.
- The IntelliJ plugin and jux-ls must implement the inspections in §P.7 in addition
  to surfacing compiler diagnostics. §P.7 inspections run in the IDE's background
  analysis pass — they do not require a full compilation.
- JuxFX (the UI framework) uses this system directly. All `Node` properties are
  declared with `{ get; set; }` in PascalCase and are therefore observable with zero
  additional infrastructure.

*End of Observable Properties addendum.*
