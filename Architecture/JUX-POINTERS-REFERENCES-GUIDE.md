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
| Call a C function, `malloc`/`free`           | `unsafe native` + `@extern`  | yes  ✅   |

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

## 5. C/C++ interop, allocation, and free/delete — §L.7–L.8 ✅ (C), 📐 (C++)

The C FFI binding layer is **implemented**: you can declare, link, and call C
functions, marshal `String`/`char`, pass `out` parameters and `@layout(c)`
structs/enums, call variadic functions, and `@export` Jux functions back to C.
Every snippet below has a runnable counterpart under `examples/ffi_*.jux`
(`./juxc.exe --run examples/ffi_strings.jux`). C++ (`autocxx`) and header
`bindgen` are the remaining 📐 pieces.

### 5.1 Declaring and calling foreign functions ✅
A `@extern(lib = "…") unsafe native { … }` block declares C functions. Each one
is implicitly `unsafe` to call, so the call must sit in an `unsafe { }` context
(an unguarded call is **E0506**). FFI-incompatible signatures (a class, generic,
array, collection, or `throws`) are rejected with **E0508**.

```jux
@extern(lib = "c")
unsafe native {
    void* malloc(ulong size);
    void  free(void* p);
    void  memset(void* p, int byte, ulong n);
}

public void main() {
    unsafe {
        void* p = malloc(64);
        if (p != null) {
            memset(p, 0, 64);
            free(p);
        }
    }
}
```

The Jux types map straight onto the C ABI (`int` to the target word `isize`,
`ulong` to `u64`, `byte*` to `*mut i8`, `void*` to `*mut c_void`, a `void`
return is omitted). See `examples/ffi_strings.jux`.

### 5.2 String and char marshalling ✅
`String` crosses the boundary as a C `const char*` automatically, in both
directions. There is **no `CString` type** in Jux. Outbound, the compiler builds
a NUL-terminated temporary that lives across the call; inbound, it copies the C
buffer into an owned Jux `String` (lossy UTF-8; the C buffer is read, never
freed; a null pointer becomes the empty string, or `null` for a `String?`
return). A `char` maps to a C `char` (one byte), converted at the call site.

```jux
@extern(lib = "c")
unsafe native {
    i32    puts(String s);        // Jux String  -> const char*
    String getenv(String name);   // const char* -> Jux String
    i32    toupper(char c);       // Jux char    -> C char
}

public void main() {
    unsafe {
        puts("hello from Jux");
        String path = getenv("PATH");           // copied out of C
        print($"PATH has ${path.len()} chars");
    }
}
```

### 5.3 `out` parameters ✅
An `out T` parameter is a place the C callee writes through. It lowers to
`*mut T`; the call passes the address of your local automatically, and the
write is visible after the call. Composes with `String`/`char` marshalling.

```jux
@extern(lib = "kernel32")
unsafe native {
    i32 QueryPerformanceCounter(out long counter);
}

public void main() {
    unsafe {
        long ticks = 0;
        QueryPerformanceCounter(out ticks);     // C fills `ticks`
        print($"ticks=$ticks");
    }
}
```

### 5.4 `@layout(c)` value structs ✅
A `@layout(c) struct` lowers to a flat `#[repr(C)]` value type (declaration-order
fields, copied on assignment, direct field access), the memory shape a C function
expects. It can be passed by value or filled through a pointer (`out P` / `P*`).
A `@layout(c)` struct with no constructor gets an implicit positional one. The
fields must be C-compatible (primitive, raw pointer, or another `@layout(c)`
struct); a `String`/class field, or `@layout(c)` on a `class`, is **E0509**.

```jux
@layout(c)
struct POINT { i32 x; i32 y; }

@extern(lib = "user32")
unsafe native {
    i32 GetCursorPos(out POINT p);
}

public void main() {
    unsafe {
        POINT p = new POINT(0, 0);
        GetCursorPos(out p);                     // C writes p.x / p.y
        print($"cursor=(${p.x}, ${p.y})");
    }
    POINT a = new POINT(1, 2);
    POINT b = a;     // value copy
    a.x = 9;         // does not touch b  (b.x stays 1)
}
```

See `examples/ffi_struct.jux`.

### 5.5 `@layout(c, repr = "…")` C enums ✅
A C enum (an integer constant) is a regular Jux enum with no payloads plus
`@layout(c, repr = "…")`. It lowers to a flat `#[repr(<repr>)]` integer enum with
the explicit discriminant on each variant. Cast it to its repr (`s as i32`) to
hand to or receive from C; it is also accepted directly as a foreign param/return
type. A payload-carrying variant under `@layout(c)` is **E0509**; an explicit
discriminant on a *non*-`@layout(c)` enum is **E0510** (it would otherwise be
silently dropped).

```jux
@layout(c, repr = "i32")
enum HttpStatus { Ok = 200, NotFound = 404, ServerError = 500 }

public void main() {
    HttpStatus s = HttpStatus.NotFound;
    i32 code = s as i32;                         // 404
    print($"status=$s code=$code");
}
```

See `examples/ffi_enum.jux`.

### 5.6 C variadic functions ✅
A foreign signature may end with `...` to call a C variadic function. The fixed
parameters type-check normally; any number of trailing arguments follow. A
trailing String *literal* is marshalled to `const char*` like a fixed `String`
parameter; other trailing args (ints, floats, pointers) pass by value. A variadic
must have at least one fixed parameter (**E0508** otherwise).

```jux
@extern(lib = "legacy_stdio_definitions")   // on Linux/macOS use lib = "c"
unsafe native {
    int printf(String fmt, ...);
}

public void main() {
    unsafe {
        printf("hi %s, %d + %d = %d\n", "world", 2, 3, 5);
    }
}
```

See `examples/ffi_variadic.jux`. (On Windows, `printf` lives in
`legacy_stdio_definitions`; `msvcrt`'s `printf` is an inline-only stub.)

### 5.7 Linking custom libraries — `[ffi.*]` in `jux.toml` ✅
System libraries link via the `@extern(lib = "…")` name directly. For a custom
library, describe it once in `jux.toml` and the generated `build.rs` emits the
link directives (`cargo:rustc-link-search` / `-link-lib`). The `@extern` block's
own `#[link]` is then dropped to avoid a double link.

```toml
[ffi.mylib]
lib       = "mylib"
lib_path  = "vendor/mylib/lib"
linkage   = "static"          # static | dynamic | framework
extra_libs = ["z"]
```

See JUX-BUILD-SYSTEM-ADDENDUM §B.14.7.

### 5.8 free / delete — call the foreign deallocator from `drop` ✅
Jux has **no `delete`/`free` keyword**. You free memory by calling the foreign
deallocator (`free` for C, a `delete` wrapper for C++) inside `unsafe`,
idiomatically from the owning class's **`drop { }`** destructor (§6.6 / §S.5), so
cleanup is automatic and deterministic:

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

Because `delete` is muscle memory for C++/Java/JS programmers, the compiler
catches a `delete p;` statement and turns it into a *guided* error: writing
`delete <thing>;` (a second operand after the word, e.g. `delete p;`,
`delete this.buf;`, `delete *p;`) reports **E0507** with a message that points
you at the `drop { }` + foreign-`free` model above. `delete` is still a usable
identifier everywhere else (`delete(x)`, `delete = v`, `delete.run()` never trip
E0507). ✅

### 5.9 `@export` — calling Jux from C ✅
`@export` gives a Jux free function C linkage so C (or anything with a C FFI) can
call it. Plain `@export` uses the Jux name as the C symbol; `@export(name = "…")`
sets a custom one. The signature must be C-compatible (primitive, raw pointer,
`@layout(c)` struct/enum, or `String`); it may not be generic, `async`, `unsafe`,
or `throws` (**E0508**). A `String` parameter/return is marshalled by a generated
wrapper: inbound C strings are copied into Jux Strings, and a returned String is
handed back via `into_raw` (the C caller owns that buffer; it is not freed on the
Jux side, mirroring the inbound rule in §5.2).

```jux
@export
public int jux_add(int a, int b) { return a + b; }

@export(name = "jux_greet")
public String greet(String name, int times) {
    return $"Hello $name (x$times)";
}
```

Build a `[lib]` crate with `crate-type = ["cdylib"]` / `["staticlib"]` to expose
these symbols as a `.dll` / `.so` / `.a`. See `examples/ffi_export.jux`.

### 5.10 FFI diagnostics quick reference
| Code  | Fires when…                                                      |
|-------|------------------------------------------------------------------|
| E0506 | an `unsafe` op (`&`, `*`, foreign call) is used outside `unsafe`  |
| E0507 | a `delete <expr>;` statement is written (use `drop`/foreign-free) |
| E0508 | an `@extern`/`@export` signature uses a non-C-compatible type     |
| E0509 | `@layout(c)` on a class, a non-C field, or a payload C-enum variant |
| E0510 | an explicit enum discriminant on a non-`@layout(c)` enum           |

### 5.11 Other `unsafe`-only tools
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
| `&x` address-of, `*p` deref (unsafe, E0506 otherwise) | ✅ |
| pointer arithmetic, `p[i]`, pointer⇄integer casts     | ✅ |
| `unsafe { }` blocks, `unsafe` functions               | ✅ |
| `drop { }` destructors                                | ✅ |
| `unsafe native` + `@extern` (declare/link/call C)     | ✅ |
| `String`/`char` marshalling, `out` params             | ✅ |
| `@layout(c)` value structs, `@layout(c, repr)` C enums | ✅ |
| C variadics (`...`), `[ffi.*]` custom-lib linking     | ✅ |
| `@export` Jux→C (incl. `String` marshalling)          | ✅ |
| foreign `free`/`delete`, `malloc` via FFI             | ✅ |
| header `bindgen`, C++ via `autocxx`                   | 📐 |
| function pointers `fn(...)->R` as C callbacks          | 📐 |
| `transmute`, inline `asm`, `Volatile.atAddress`       | 📐 |

The ✅ rows work today: declaring, linking, and calling C functions with full
`String`/`char`/`out`/struct/enum/variadic marshalling, and exporting Jux
functions back to C. The remaining 📐 rows are header `bindgen` and C++
(`autocxx`), the next FFI milestone (§L.7–L.8, JUX-BINDGEN-ADDENDUM).
