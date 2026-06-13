# Jux

**A Java/C#-flavored language that transpiles to Rust.** No VM, no garbage collector at runtime. Your code compiles down to a native machine-code binary through `rustc`, and you get Rust's optimizer and safety guarantees for free.

> ⚠️ **This is experimental. It is a hobby, a personal project, a work in progress.**
> It will have bugs. The docs will sometimes contradict each other because there's
> a *lot* of them and I'm one person. Things will break, change, and get rewritten.
> If that scares you, come back in a year. If it sounds fun, keep reading.

---

## Where this came from

I started sketching this idea back in **2019**, right when COVID hit and the days
stuck at home got long and boring. I wanted a language that *felt* like the ones I
already knew, Java and C#, but that didn't drag a virtual machine around with it.

I've tried to build it more than once. First in **Java**. Then in **Dart**. Both
times I learned a lot and both times I hit a wall. This is the **third attempt**,
and this time I went with **Rust** as the foundation, because honestly it's one of
the best tools out there right now for this kind of work. It compiles to fast
native code, the borrow checker catches a whole category of bugs before they ship,
and the crate ecosystem is enormous.

So instead of fighting Rust, Jux **stands on top of it**. Jux code is translated to
readable Rust source, and then `rustc` does the heavy lifting. That means Jux gets
the good parts of Rust under the hood while wearing a syntax that someone coming
from Java, C#, or even Rust itself can pick up without much friction.

**A few things I want to be straight about:**

- I'm **not an expert**. I'm a developer who's been at this a while and decided to
  stop wishing this language existed and actually build it.
- This is built by a **solo dev** (me) with help from **AI**. To be clear: the AI
  helps with research and grinding through steps, but it does **not** make the
  decisions. Every direction, every design call, every "no, do it this way" is
  mine. I drive; it assists.
- I'm **not trying to replace Rust** or compete with anything. This is a hobby that
  *might* turn into something useful for other people too. That's the whole ambition.
- It's made with love and a stupid amount of dedication, and even if it sucks right
  now, I'm genuinely happy it's at the stage it's at.

---

## The pitch, in one breath

Write in a familiar, Java-shaped language. Get a native binary. Use Rust's standard
library as your standard library. Pull in any crate on crates.io and call it with
Jux syntax. Pull in other Jux libraries straight from GitHub as dependencies. Build
real frameworks with annotations. Talk to C and C++ through FFI. And let `rustc`
optimize all of it to the metal.

That's the goal. Some of it works today, some of it is half-built, some of it is
still on paper. I'll be honest below about which is which.

---

## A taste of Jux

If you've written Java or C#, none of this needs a tutorial:

```java
public abstract class Animal {
    public String name;
    public Animal(String name) { this.name = name; }
    public abstract String sound();
}

public interface Tagged { String tag(); }

public class Dog extends Animal {
    public Dog(String name) { super(name); }
    public String sound() { return "Woof"; }
    public String fetch() { return "fetched"; }
}

public class Cat extends Animal implements Tagged {
    public Cat(String name) { super(name); }
    public String sound() { return "Meow"; }
    public String tag()    { return "cat-tag"; }
}

public void main() {
    Animal a = new Dog("Rex");
    a.name = "Max";                 // field access through a base reference
    print(a.sound());               // virtual dispatch, prints "Woof"

    Dog d = a as Dog;               // explicit downcast
    print(d.fetch());

    Animal b = new Cat("Felix");
    if (b => Cat) {                 // `=>` is the instanceof / type-test operator
        Cat c = b as Cat;
        print($"${c.name} says ${c.sound()} / ${c.tag()}");
    }
}
```

Familiar shape, but the semantics are Jux's own, and all of it compiles straight to
native code through Rust. (This is a real example;
see [`examples/downcast_typetest.jux`](examples/downcast_typetest.jux).)

---

## More of the language

A tour of the stuff that makes Jux fun to write. Everything below is real,
compiling syntax (most of it lifted straight out of [`examples/`](examples/)).

### Structs and generics

```java
public struct Vec2 {
    public double x = 0.0;        // fields need a default or constructor assignment
    public double y = 0.0;
    public double lengthSquared() { return x * x + y * y; }
}

// A generic container, instantiated with a turbofish or by inference.
public class Box<T> {
    private T value;
    public Box(T value) { this.value = value; }
    public T get() { return this.value; }
}

// Bounded type parameter: T must be an Animal AND implement Speaks.
public class Holder<T extends Animal & Speaks> {
    public T pet;
    public Holder(T pet) { this.pet = pet; }
    public String describe() { return this.pet.voice(); }
}

var b = new Box<int>(42);        // explicit type argument
var v = new Vec2();              // v.x = 3.0; v.y = 4.0; ...
```

### Operator overloading

Overload arithmetic, equality, hashing, and the string conversion. `operator==`
must be paired with `operator hash` (the compiler enforces it with `E0931`).

```java
public class Money {
    public int cents;
    public Money(int cents) { this.cents = cents; }

    public Money  operator+(Money other) { return new Money(this.cents + other.cents); }
    public Money  operator-(Money other) { return new Money(this.cents - other.cents); }
    public bool   operator==(Money other) { return this.cents == other.cents; }
    public int    operator hash()         { return this.cents; }
    public String operator string()       { return $"$${this.cents}"; }
}

var total = new Money(150) + new Money(50);   // both operands stay usable afterward
print($"total=$total");                       // total=$200
```

### Indexers: overloading `[]`

You can also overload the subscript operator `[]`, the same way C# has indexers
and C++ has `operator[]`. It comes as a **read/write pair**: `operator[]` defines
what `w[i]` returns, and `operator[]=` defines what `w[i] = v` does. That lets a
class expose clean array-style access while keeping its storage private.

```java
import rust.std.Vec;

public class Wallet {
    private Vec<int> slots;                    // private backing store
    public Wallet() {
        this.slots = new Vec<int>();
        this.slots.push(10);
        this.slots.push(20);
    }

    // index read:  evaluated for  w[i]
    public int  operator[](int i)          { return this.slots[i]; }
    // index write: evaluated for  w[i] = v
    public void operator[]=(int i, int v)  { this.slots[i] = v; }
}

public void main() {
    var w = new Wallet();
    print(w[0]);          // 10    calls operator[]
    w[1] = 99;            //       calls operator[]=
    w[0] += w[1];         // both! reads w[0] and w[1], then writes back
    print(w[0]);          // 109
}
```

That last line is the fun one: a compound assignment through an indexer fires
**both** operators in a single statement, the getter to read and the setter to
write back. The `operator[]` / `operator[]=` bodies lower to inherent methods, and
`w[i]` at the call site maps onto Rust's `Index`/`IndexMut` shape, so the emitted
Rust still reads naturally.

### Type aliases, `sizeof`, `typeof`

```java
public type UserId = long;             // transparent alias
public type Predicate = (int) -> bool; // function-type alias

public void main() {
    UserId id = 42;
    print(sizeof(UserId));             // 8   (compile-time size query)
    print(typeof(id));                 // long

    Predicate even = (n) -> n % 2 == 0;
    print(even(10));                   // true
}
```

### Grouped imports

Pull several names from one package with brace syntax, just like the frameworks
Jux is built to host:

```java
import rust.std.{HashMap, HashSet};
import juxweb.{Server, Controller, Route, PathParam};
```

### Properties, with observers

Properties read and write like fields, but they can be computed, bound to one
another, and observed. This is one of Jux's signature features.

```java
public class Person {
    public String First { get; set; } = "";
    public String Last  { get; set; } = "";
    // Computed property: re-fires whenever First or Last changes.
    public String FullName { get -> First + "/" + Last; };
}

public class Source { public int Value { get; set; } = 10; }
public class Mirror {
    public int Shown { get; set; } = 0;
    public Mirror(Source s) { this.Shown.bind(s.Value); }   // one-way binding
}

var p = new Person();
p.FullName.observers.attach((old, now) -> print($"name: $old -> $now"));
p.First = "Ada";                  // fires the observer
p.Last  = "Lovelace";             // fires again

var s = new Source();
var m = new Mirror(s);            // m.Shown starts synced to s.Value
s.Value = 42;                     // m.Shown now follows to 42

a.Value.bindBidirectional(b.Value);   // two-way; either side updates the other
a.Value.unbind();                     // break the binding
```

### Async, channels, and spawned tasks

`async`/`await` lower to real Rust futures; `spawn` launches a concurrent task and
`Channel<T>` gives you a bounded producer/consumer pipe.

```java
async void pipeline() {
    var ch = new Channel<int>(4);
    spawn(async () -> {
        for (var i : 1..=5) { await ch.send(i * 10); }
        ch.close();
    });

    var total = 0;
    while (true) {
        var item = await ch.receive();    // null once closed and drained
        if (item == null) { break; }
        total = total + item!!;           // `!!` unwraps a nullable
    }
    print(total);                         // 150
}

public void main() { block_on(pipeline()); }
```

### Real threads: workers and atomics

When you want genuine multi-core parallelism (not just cooperative tasks),
`Worker.spawn` runs a closure on a real OS thread and hands you back a `Task` you
can `await`. Shared state goes through atomics like `AtomicInt`, which are safe to
hand to several workers at once.

```java
import jux.std.concurrent.AtomicInt;

public int crunch(String tag, int iters) {
    var acc = 0;
    for (var i : 0..iters) { acc = (acc + i) * 7 % 9973; }
    print($"  [worker $tag] acc=$acc");
    return acc;
}

public async void main() {
    // Fan CPU-bound work out across real threads, then gather the results.
    final var a = Worker.spawn(() -> crunch("A", 1_000_000));
    final var b = Worker.spawn(() -> crunch("B", 1_000_000));

    // One shared, thread-safe counter that every worker bumps.
    var hits = new AtomicInt(0);
    final var c = Worker.spawn(() -> {
        for (var i : 0..1000) { hits.fetchAdd(1); }   // atomic increment
        return 0;
    });

    final int ra = await a;
    final int rb = await b;
    await c;

    print($"results: A=$ra B=$rb");
    print($"hits = ${hits.load()}");      // 1000
}
```

`Worker.spawn` is true preemptive parallelism backed by OS threads; `AtomicInt`
(and friends, with explicit `MemoryOrder`) lower to `Arc<Atomic*>`, so the handle
the workers share really is one counter.

### Memory: `drop`, `weak`, and `ref`

Jux has no tracing garbage collector. Class instances are reference-counted (that
`Rc<RefCell>` lowering from earlier), and three constructs give you direct control
over lifetime and sharing.

**`drop { }` is a deterministic destructor.** It runs at scope exit for a local,
and for a class instance exactly once, when the last strong reference is released.
No finalizer queue, no nondeterminism.

```java
public class Resource {
    public String name;
    public Resource(String name) { this.name = name; print("open " + name); }
    drop { print("close " + name); }      // `this` is in scope here
}

public void main() {
    var a = new Resource("a");
    var b = a;                  // a SECOND handle to the same resource
    print("using " + b.name);
}   // "close a" prints once here, when the last handle dies
```

**`weak` breaks reference cycles.** A `weak` reference doesn't bump the refcount,
so it never keeps its target alive. Promote it to a strong reference with `.get()`,
which returns `T?` (null if the target is gone). This is how a `Parent <-> Child`
back-reference avoids leaking without a GC.

```java
public class Child {
    private weak Parent parent;             // no refcount contribution
    public void attach(Parent p) { this.parent = p; }
    public void callUp() {
        var p = this.parent.get();          // Parent?  promote weak -> strong
        if (p != null) { p.greet(); } else { print("(no parent)"); }
    }
}
```

**`ref` gives you a shared, writable handle to a value type.** Normally primitives
and value types copy; a `ref` binding (or `ref` parameter) aliases the *same* cell,
so a write is visible through every handle, including the caller's.

```java
void bump(ref int n) { n += 5; }            // mutates the CALLER's variable

public void main() {
    ref int n = 10;
    ref int m = n;                           // m aliases n's cell
    bump(n);
    print(m);                                // 15 (same cell throughout)
}
```

> Not yet: `unsafe { }` blocks and raw pointers (`int*`, `(byte*) buf`) are
> specced but still placeholder, and parked behind the C/C++ FFI work. Don't reach
> for them yet.

---

## What works today

The pipeline runs end to end: **lex, parse, resolve, typecheck, lower-to-Rust,
`cargo build`, run.** Roughly anything in [`examples/`](examples/) compiles and
runs. That currently includes:

- **Classes:** fields, constructors, methods, `static`/`final`, visibility,
  bare field access (`f` is `this.f`), and C#-style **properties**
  (`{ get; set; }`, expression-bodied with `->`).
- **Inheritance & polymorphism:** `extends`, `super(...)`, overrides, abstract
  classes, `sealed`/`non-sealed`, virtual dispatch, downcasts (`as` / `(T)`), and
  the `=>` instanceof / type-test operator. Classes are **shared references**
  (Java semantics), not values.
- **Interfaces:** default methods, static methods, constants. Single-class,
  multi-interface inheritance, like Java.
- **Generics:** `class A<T>`, bounded type params, wildcards (`? extends`,
  `? super`), const generics (`<int N>`), explicit type arguments.
- **Enums + `match`** with exhaustiveness checking and payload binding.
- **Records**, **lambdas & method references**, **operator overloading**.
- **Annotations** (case-insensitive built-in lookups).
- **String interpolation:** `$"hello ${name}"`.
- **Observable properties:** `observer<T>`, binding, bidirectional binding.
- **Async/streams, a testing framework, exceptions** (`try`/`catch`/`finally`).
- **Concurrency:** `async`/`await`, `spawn` tasks, `Channel<T>`, real-thread
  `Worker.spawn`, and atomics (`AtomicInt`/`AtomicLong` with `MemoryOrder`).
- **Memory control:** `drop { }` deterministic destructors, `weak` references
  (cycle-breaking, `.get()` to promote), and `ref` bindings/params (shared,
  writable handles to value types). No tracing GC.
- **Multi-file workspaces:** cross-file `import`s, package-private visibility,
  and `jux.toml`-driven multi-module project builds with per-module binary
  metadata (version, author, icon).

## What's stubbed or in progress

- `jux new` / `jux test` CLI subcommands are still stubs.
- `rust.std` compile coverage is partial: construction and method calls work;
  free functions, traits/operators, and the full type mapping are being filled in.
- **C/C++ FFI** is specced and is a priority, but deferred for now.
- Real tuple/pointer/unsafe interop syntax is still placeholder.

---

## How Jux uses Rust (the part I'm proud of)

### The standard library is Rust's standard library

There's no separate "Jux runtime library" to reinvent. **Rust's `std` is the Jux
`std`**, and any Rust crate is fair game, surfaced in Jux syntax.

```java
import rust.std.PathBuf;

var p = new PathBuf();   // lowers to std::path::PathBuf::new()
p.reserve(16);           // camelCase method maps to the real snake_case one
```

Hover, autocomplete, and go-to-definition over `std` and your project's crates are
generated **on demand** from the installed toolchain's rustdoc JSON
(`juxc-bindgen`): nothing is hand-curated, so it tracks whatever Rust version you
actually have. Collections are Rust's collections under their real names: `Vec`,
`HashMap`, `HashSet`, `VecDeque`.

### Dependencies: crates *and* Jux libraries

- **Rust crates** from crates.io, consumed and called with Jux syntax.
- **Jux libraries straight from GitHub.** Point at a repo (with branch / tag /
  rev, or a bare-URL shorthand), and `jux` resolves and caches it under `~/.jux`.
  Cross-compilation via `--target <triple>` is supported.

### Annotations for frameworks

Annotations are first-class and the plan is to lean on them hard, so people can
build clean, declarative frameworks on top of Jux the way Spring or ASP.NET did
for their ecosystems.

### C / C++ FFI

Full C/C++ interop is on the roadmap and matters a lot to me. The path is known
(link via `build.rs`, bind via bindgen/autocxx). It's deferred behind other work
right now, but it's coming.

---

## How the "borrow checker" works, and how we lower to Rust

This is the question I get most, so here's the honest mechanical answer.

**Jux does not ask you to write lifetimes, `&`, `&mut`, or `.clone()` by hand.**
You write Java-shaped code. The compiler's job is to translate that into Rust that
**passes `rustc`'s borrow checker on the first try**, without you ever thinking
about ownership. So the "borrow checker" in Jux is really a **lowering strategy**:
an ownership analysis in the backend that decides, for every value, how it should
be represented and shared in the emitted Rust so that the program both *means* what
you wrote and *compiles* under Rust's rules.

The core decisions it makes:

- **Class instances are shared, mutable references**, exactly like objects in Java
  or C#. They lower to `Rc<RefCell<...>>`. When you pass an object around or store
  it in two places, the backend inserts an `Rc::clone` (a cheap refcount bump, what
  I call a *share-clone*) so both sides hold the same live object, not a copy.
  Mutation goes through `RefCell`, so two references see each other's changes. Java
  semantics, achieved with safe Rust.
- **Value types stay values.** Primitives, small structs, and records lower to
  plain Rust values and move/copy the way Rust naturally wants. No `Rc` overhead
  where it isn't needed.
- **`ref` bindings** (shared references to value-typed locals, params, and fields)
  also lower to `Rc<RefCell<...>>` when you explicitly ask for shared mutation.
- **The backend hoists and reshapes** the emitted code to keep `rustc` happy:
  receiver-mutation calls get hoisted so a `&mut` borrow doesn't overlap an
  argument evaluation; lambda captures are share-cloned; `!Send` statics become
  `thread_local!`; recursive class shapes get wrapped; container and nullable
  fields share-clone on read. These are the kinds of borrow conflicts you'd
  normally hit by hand in Rust, and Jux resolves them for you at lowering time.

The result is **human-readable Rust**: it's meant to look like something a person
would have written, with sensible parentheses, no needless `let mut` or type
suffixes, rustfmt-style braces. You can open the emitted crate and follow it.

And because the final artifact is just Rust, **you get the entire Rust optimization
pipeline** (LLVM, inlining, monomorphization, dead-code elimination, release-mode
codegen) applied to your Jux program. Jux doesn't try to be a fast
compiler-of-fast-code on its own; it hands a clean Rust crate to the best
optimizing backend already out there and gets out of the way.

```
  your.jux  ->  juxc  ->  readable .rs crate  ->  cargo / rustc  ->  native binary
                  |                                     |
           ownership lowering                   LLVM optimizes
        (share-clones, RefCell,                  everything
         hoists; no borrow errors)
```

---

## Getting started

### 0. You need Rust

**A working Rust toolchain is required.** Jux compiles *through* `rustc`, so
`cargo` and `rustc` must be on your `PATH`. Install from <https://rustup.rs>. The
repo pins a stable toolchain (`rust-toolchain.toml`), which `rustup` honors
automatically.

```sh
rustc --version
cargo --version
```

### 1. Build the toolchain

```sh
git clone https://github.com/xdsswar/juxlang
cd juxlang
cargo build --release -p juxc -p jux -p juxc-lsp
```

You get three binaries in `target/release/`:

| Component  | What it is                                                 |
|------------|------------------------------------------------------------|
| `juxc`     | The compiler (file-level: compile / build / run)           |
| `jux`      | The project tool (reads `jux.toml`, resolves deps)         |
| `juxc-lsp` | The language server (IDE diagnostics / hover / completion) |

### 2. Put the binaries in a folder and set `JUX_HOME`

The tools and the IntelliJ plugin look for `juxc` / `juxc-lsp` in this order:
an explicitly configured path, then **`$JUX_HOME`**, then your `PATH`. The
recommended setup is to drop the executables straight into one folder and point
`JUX_HOME` at it (no `bin/` subfolder; the binaries sit directly in `JUX_HOME`).

**Windows (PowerShell):**

```powershell
$JuxHome = "C:\Tools\jux"
New-Item -ItemType Directory -Force -Path "$JuxHome" | Out-Null
Copy-Item target\release\juxc.exe,target\release\jux.exe,target\release\juxc-lsp.exe "$JuxHome"

setx JUX_HOME $JuxHome
setx PATH "$env:PATH;$JuxHome"
```

**macOS / Linux (bash/zsh):**

```sh
JUX_HOME="$HOME/.jux"
mkdir -p "$JUX_HOME"
cp target/release/juxc target/release/jux target/release/juxc-lsp "$JUX_HOME/"

echo 'export JUX_HOME="$HOME/.jux"'  >> ~/.zshrc
echo 'export PATH="$JUX_HOME:$PATH"' >> ~/.zshrc
```

Open a **new** terminal afterward so the variables take effect (and restart
IntelliJ so it inherits `JUX_HOME`).

### 3. Write and run your first program

```java
public void main() {
    print("Hello, world!");
}
```

```sh
juxc hello.jux --run      # lowers to Rust, cargo-builds, runs, forwards exit code
```

The first run compiles a small Rust crate under the hood; later runs reuse the
cached build.

### 4. Install the IntelliJ plugin

The IDE plugin is a thin client: all the smart features come from `juxc-lsp`, so
install the binaries first.

```sh
cd ide/intellij-plugin
./gradlew buildPlugin        # Windows: .\gradlew.bat buildPlugin
```

The first build downloads the IntelliJ Platform and a JDK 21 toolchain
automatically. The result is `build/distributions/jux-intellij-0.0.1.zip`. Then in
your IDE:

**Settings/Preferences > Plugins > gear icon > Install Plugin from Disk...**, pick
the zip, then **restart**.

You'll get syntax highlighting, **New > Jux File** templates (Class, Interface,
Enum, Struct, Record, Annotation), diagnostics, hover types, completion, and a Run
button for any file with a `main`.

> On **IntelliJ Community**, the native LSP client is inert, so install **LSP4IJ**
> from the Marketplace and register `juxc-lsp` for the Jux file type. On
> **Ultimate / paid IDEs** it's automatic.

📖 **Full step-by-step install (with troubleshooting): [`INSTALL.md`](INSTALL.md).**

---

## Project config: `jux.toml`

`jux` is the cargo-equivalent project tool, and `jux.toml` is its manifest. It names
your binary, carries the metadata that gets baked into the executable (version,
author, company, icon, copyright), and lists dependencies, whether they're other
Jux packages, GitHub repos, or Rust crates.

```toml
[package]
name    = "it.xss.myapp"                 # reverse-DNS package name
version = "0.1.0"
edition = "2026"
description = "A little Jux app."
authors = ["XDSSWAR <you@example.com>"]
license = "Apache-2.0"

# Baked into the produced executable's resource block (Windows version-info, icon).
icon      = "assets/app.ico"
company   = "XTREME SOFTWARE SOLUTIONS"
copyright = "© 2026 XSS"

[[bin]]
name = "myapp"                           # the output binary name -> myapp.exe
main = "it.xss.Main"                     # entry file by dotted path: src/it/xss/Main.jux

[dependencies]
# Another Jux package, straight from GitHub (tracks the default branch):
"it.xss.toolkit" = "https://github.com/xdsswar/toolkit"
# ...or pinned to a tag / branch / rev:
"com.acme.json"  = { git = "https://github.com/acme/json", tag = "v1.4.2" }
# A Rust crate from crates.io, used with Jux syntax:
"rust.serde_json" = "1.0"
# A C library (FFI), once that lands:
"c.sqlite3"       = { lib = "sqlite3", header = "sqlite3.h" }
```

Then `jux run` builds the whole thing and produces `myapp.exe` with your icon and
version metadata embedded.

---

## The two binaries

- **`juxc`** is the compiler. Works on individual files or directories. Doesn't read
  `jux.toml` or resolve dependencies. Exposes `--run`, `--build`, `--name`,
  `--release`, `--emit-dir`.
- **`jux`** is the project tool (the cargo-equivalent). Reads `jux.toml`, resolves
  dependencies, and dispatches `juxc`. This is what you use day to day; `juxc` is
  invoked by `jux`, by the language server, and by foreign build systems.

```sh
juxc examples/hello.jux                 # lower to Rust only
juxc examples/hello.jux --run           # compile + build + run
juxc examples/multifile --run           # compile a whole directory as one workspace
jux  run examples/hello.jux             # via the project tool
jux  run --release examples/hello.jux   # optimized emitted program
```

---

## Repository layout

```
juxlang/
├── Architecture/                # the language specification (the contract)
│   ├── JUX-LANG-V1.md           # consolidated dossier
│   └── JUX-*-ADDENDUM.md        # 20+ normative addenda
├── examples/                    # .jux programs; every one compiles & runs
├── crates/
│   ├── juxc-source/             # source files, spans, positions
│   ├── juxc-diagnostics/        # diagnostic types, E-codes, JSON output
│   ├── juxc-lex/                # lexer
│   ├── juxc-ast/                # AST types
│   ├── juxc-parse/              # parser
│   ├── juxc-resolve/            # name resolution
│   ├── juxc-tycheck/            # type checking
│   ├── juxc-backend-rust/       # lowering to Rust (the ownership analysis lives here)
│   ├── juxc-bindgen/            # rustdoc JSON to Jux-syntax stubs
│   ├── juxc-lsp/                # language server
│   └── juxc-driver/             # phase orchestration + project/workspace builds
├── ide/intellij-plugin/         # IntelliJ plugin (Java-style PSI)
└── bin/{juxc,jux}/              # the two binary entry points
```

---

## A note on the spec

There's a full specification under [`Architecture/`](Architecture/): `JUX-LANG-V1.md`
plus 20+ addenda covering grammar, the type system, the ABI, diagnostics, the build
system, async, exceptions, annotations, class representation, and more. The spec is
**authoritative**: behavior should trace to a clause. If something isn't decided
yet, the spec gets updated before the code does. Because there's so much of it, you
*will* find inconsistencies here and there. That's the cost of one person
maintaining a large design surface, and I clean them up as I find them.

---

## License & ownership

The code is **free and open for everyone**: open source, use it, learn from it,
build on it. That said, **I am the sole owner of the Jux language** itself (the
design, the name, the direction). Distributed under **Apache-2.0**.

---

*Built solo, with love, by [XDSSWAR](https://github.com/xdsswar), XTREME SOFTWARE
SOLUTIONS. Third time's the charm.* 🚀
