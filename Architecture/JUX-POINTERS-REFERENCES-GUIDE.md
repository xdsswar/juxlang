# Jux Pointers, References, Identity & C/C++ Interop — Consolidated Guide

> Status legend: ✅ implemented in `juxc` today · 📐 specified, implementation deferred.
>
> This guide pulls together material that lives across several addenda so the
> whole pointer/reference/memory story is in one place. The authoritative
> specs are **JUX-LAYOUT-ABI-ADDENDUM §L.5–L.8** (unsafe, raw pointers, FFI),
> **JUX-MISSING-DEFS-ADDENDUM §M.13** (`ref` bindings), and **JUX-LANG-V1 §5.5
> / §6 / §8**. Where this guide and a spec disagree, the spec wins.

---

## 1. The two worlds: safe references vs raw pointers

Jux deliberately separates **safe references** (everyday Jux, no `unsafe`) from
**raw pointers** (C/C++ interop, `unsafe`-only). You reach for raw pointers
*only* when crossing into C/C++; ordinary Jux code never needs them.

| You want…                                   | Use                          | `unsafe`? |
|---------------------------------------------|------------------------------|-----------|
| Pass an object around (it's already shared) | just pass it (Java semantics)| no        |
| A named alias to a value type               | `ref` binding (§M.13)        | no   ✅   |
| Ask "are these the same object?"            | `===` / `!==`                | no   ✅   |
| The raw memory address of something         | `&x` → `T*`                  | yes  ✅   |
| Read/write through an address               | `*p`, `p[i]`                 | yes  ✅   |
| Call a C/C++ function, `malloc`/`free`       | `unsafe native` + `@extern`  | yes  📐   |

---

## 2. Safe references (no `unsafe`)

### 2.1 Object reference semantics ✅
A Jux class instance is a **shared reference** (Java semantics — lowered to
`Rc<RefCell<…>>`). Assigning or passing an object does not copy it; both names
see the same object. This is the default; nothing special to write.

### 2.2 `ref` bindings — §M.13 ✅
`ref` gives a **safe** named alias to a *value type* (locals, params, fields),
without taking a raw address:

```jux
int total = 0;
ref int acc = total;     // acc aliases total
acc = acc + 5;           // total is now 5
```

Backed by `Rc<RefCell<T>>`, fully borrow-checked. This is the safe answer to
"I want a reference to a value", and needs no `unsafe`.

### 2.3 Reference identity — `===` / `!==` ✅
`===` (and `!==`) compare **reference identity** — whether two expressions
denote the *same* object — independent of any `equals`/`operator==`:

```jux
var a = new Object();
var b = a;
var c = new Object();
print(a === b);   // true  — same object
print(a === c);   // false — distinct objects
print(a !== c);   // true
```

`==` compares by value (via `operator==`); `===` compares identity. Use `===`
when you mean "the very same instance".

> "Get the address of an object" without `unsafe`: Jux's safe identity tool is
> `===`, not a numeric address. A raw numeric address requires `&` and `unsafe`
> (§3) — by design, because a bare address escapes the safety guarantees.

---

## 3. Raw pointers (`unsafe`-only) — §L.6 ✅

Raw pointers are the C/C++ interop tool. They are gated behind `unsafe`: a
prefix `&`, a `*p` dereference, pointer arithmetic, or `p[i]` **outside**
`unsafe` is a compile error (**E0807**). Pointer *values* can be stored and
passed around in safe code; they're just opaque until you enter `unsafe`.

```jux
public void main() {
    int n = 42;
    unsafe {
        int* p = &n;        // address-of  → *mut int   (E0807 outside unsafe)
        int v = *p;         // dereference (read)        → 42
        *p = 100;           // dereference (write)       → n == 100
        int* q = p + 1;     // pointer arithmetic (steps of sizeof(int))
        long d = q - p;     // pointer difference
        int third = p[3];   // p[i] ≡ *(p + i)
        ulong a = p as ulong;   // pointer → integer
        int* p2 = a as int*;    // integer → pointer
        if (p == null) { }      // null compare (null is the only T* literal)
    }
}
```

Key properties of `T*` (§L.6.1):
- A **primitive**, target-word-sized; **nullable by default** (`null` is a `T*`).
- A plain bit value — copying never touches `T` or any refcount.
- **Not borrow-checked** — that freedom is exactly why dereferencing is `unsafe`.

`&` is reserved *exclusively* for address-of inside `unsafe`; Jux has no
implicit borrow-reference syntax (it uses inferred borrows per §6). ✅

**`&obj` on a class object** (§L.6.5) gives a `T*` aimed at the **inner object
value**, not at the `Rc<RefCell>` handle that owns it, so it is the address C/C++
interop expects. It is a *borrowing, non-owning* pointer: it does not bump the
refcount, does not keep `obj` alive, and steps around the normal borrow checks
(that is exactly why it needs `unsafe`). It lowers to `obj.as_ptr()`. For plain
object *identity* without `unsafe`, reach for `===` (§2), not a numeric address. ✅

Function pointers `fn(A) -> R` (distinct from closures `(A) -> R`) are the C
callback mechanism (§L.6.4) 📐.

---

## 4. `unsafe { }` blocks — §L.5 ✅

`unsafe { }` is an expression/statement block that unlocks the pointer
operations above (and `transmute`, inline `asm`, FFI calls). It does **not**
disable the borrow checker for safe values, null-checks, or bounds checks —
it only permits the specific unsafe *operations*. Keep blocks small and
justify each with a `// SAFETY:` note (house style, mirrors the spec examples).

A function may be declared `unsafe` (`public unsafe T f(...)`), meaning calling
it requires an `unsafe` context.

---

## 5. C/C++ interop, allocation, and free/delete — §L.7–L.8 📐

This is the **deferred** layer (FFI binding). The *language primitives* it
builds on (raw pointers, `&`/`*`, `unsafe`) are ✅; the binding that actually
links and calls foreign functions is 📐 (specified, not yet emitted).

### 5.1 Declaring foreign functions
```jux
@extern(lib = "c")
unsafe native {
    void* malloc(ulong size);
    void  free(void* p);
    void  memset(void* p, int byte, ulong n);
}
```
Every function in an `unsafe native` block is implicitly `unsafe` to call.

### 5.2 free / delete — call the foreign deallocator inside `unsafe`, from `drop`
Jux has **no `delete`/`free` keyword**. You free memory by calling the
foreign deallocator (`free` for C, a `delete` wrapper for C++) inside an
`unsafe` block — idiomatically from the owning class's **`drop { }`**
destructor, so cleanup is automatic and deterministic:

```jux
public final class RawBuffer {
    private byte* ptr;
    private ulong size;

    public RawBuffer(ulong bytes) throws OutOfMemoryError {
        var p = unsafe { malloc(bytes) };
        if (p == null) throw new OutOfMemoryError(bytes);
        this.ptr = unsafe { p as byte* };
        this.size = bytes;
    }

    public byte read(ulong i) {
        if (i >= size) throw new IndexOutOfBoundsException(i);
        return unsafe { ptr[i] };               // bounds-checked, then unsafe read
    }

    drop {
        // SAFETY: ptr came from malloc; we own it; this is the unique drop.
        unsafe { free(ptr as void*); }
    }
}
```

`drop { }` is Jux's destructor (§6.6 / §S.5) and runs deterministically when
the object dies — the right place to release a foreign resource. ✅ for the
`drop` block itself; the `free(...)` *call* is 📐 (needs the FFI layer).

Because `delete` is muscle memory for C++/Java/JS programmers, the compiler
catches a `delete p;` statement and turns it into a *guided* error instead of a
confusing one. Writing `delete <thing>;` (a second operand after the word, e.g.
`delete p;`, `delete this.buf;`, `delete *p;`) reports **E0507** with a message
that points you at the `drop { }` + foreign-`free` model above. `delete` is still
a usable identifier everywhere else (`delete(x)`, `delete = v`, `delete.run()`
never trip E0507). ✅

### 5.3 Other `unsafe`-only tools
- `transmute<A, B>(v)` — bit reinterpret, `sizeof<A>() == sizeof<B>()` (E0840). 📐
- `asm(...)` — inline assembly, `jux-embedded`/`jux-core` profiles only. 📐
- `Volatile<T>.atAddress(addr)` — MMIO at a runtime address. 📐

---

## 6. Implementation status at a glance

| Capability                                            | Status |
|-------------------------------------------------------|--------|
| Object = shared reference (Java semantics)            | ✅ |
| `ref` bindings (§M.13)                                | ✅ |
| `===` / `!==` reference identity                      | ✅ |
| `T*` raw-pointer type, `null` literal                 | ✅ |
| `&x` address-of, `*p` deref (unsafe, E0807 otherwise) | ✅ |
| pointer arithmetic, `p[i]`, pointer⇄integer casts     | ✅ |
| `unsafe { }` blocks, `unsafe` functions               | ✅ |
| `drop { }` destructors                                | ✅ |
| `unsafe native` + `@extern` (call C/C++)              | 📐 |
| foreign `free`/`delete`, `malloc` via FFI             | 📐 |
| function pointers `fn(...)->R` as C callbacks          | 📐 |
| `transmute`, inline `asm`, `Volatile.atAddress`       | 📐 |

The 📐 rows are the **FFI binding layer**: the design is fixed (§L.7–L.8), and
it's the natural next milestone for real C/C++ work. The ✅ rows — including
everything you need to take an address, dereference, and manage a pointer's
lifetime in a `drop` block — work today.
