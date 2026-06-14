# Jux benchmark harness

A small set of self-timing workloads for tracking Jux runtime performance, so the
optimizations in `optimizations.md` are **measured, not estimated**. Each
workload prints a parseable `RESULT <name>=<ms>` line; the runner builds them all
in `--release` (so the default Tier-0 profile applies), runs each a few times,
and reports the fastest number.

Run it before and after each optimization (Tier-1 `Rc<RefCell>` work, Tier-3
devirtualization, etc.) and compare the tables.

## Running

```pwsh
pwsh benchmarks/run.ps1                 # all benchmarks, 3 repeats
pwsh benchmarks/run.ps1 -Repeats 5      # more repeats (less noise)
pwsh benchmarks/run.ps1 -Only graph_walk
```

It resolves the compiler in this order: `$env:JUX_HOME` (your installed release
build - `<JUX_HOME>\juxc.exe` or `<JUX_HOME>\bin\juxc.exe`), then `./juxc.exe` at
the repo root, then `target/release` / `target/debug`. With `JUX_HOME` set to your
release install, it just works. Otherwise keep the root binary fresh
(`cargo build --release --bin juxc`) so the numbers reflect your latest changes.
(The juxc build mode only affects compile speed - the benchmark programs are
always built `--release` by `rustc` either way.)

## Example output

```
Jux benchmark results (--release, Tier-0 profile):

Benchmark           Metric                  Value
---------           ------                  -----
numeric_mandelbrot  work ms (min of 3)        192
alloc_churn         work ms (min of 3)        445
dispatch_poly       work ms (min of 3)        394
graph_walk          work ms (min of 3)        240
startup             process ms (min of 9)    5.42
```

(Numbers are from one Windows dev machine; treat them as a *baseline to compare
against*, not absolutes. `startup ≈ 5 ms` is the AOT-native win - no VM boot or
JIT warmup, versus tens-to-hundreds of ms for the JVM/CLR.)

## The workloads

| File | Measures | Optimization it tracks |
|------|----------|------------------------|
| `numeric_mandelbrot.jux` | Tight f64 escape-time loop, zero allocation | Raw LLVM numeric throughput (Jux's strong suit) |
| `alloc_churn.jux` | Allocate + drop millions of small objects (each `new` is an `Rc::new`) | **Tier-1** "don't wrap when not shared" (stack-allocate non-escaping objects) |
| `dispatch_poly.jux` | Virtual calls whose receiver type varies at runtime | **Tier-3** devirtualization (static dispatch for monomorphic/final/sealed) |
| `graph_walk.jux` | Traverse a 12M-node linked structure, reading a field through each reference | **Tier-1** the `Rc<RefCell>` tax: `Cell` for `Copy` fields, borrow-flag elision |
| `startup.jux` | Whole-process start → exit (timed by the runner) | AOT startup advantage; nothing to optimize, just to showcase |

Each workload accumulates a `checksum` it prints, so the optimizer can't elide the
work and so a change that breaks semantics is caught (the checksum shifts).

## Known limitations surfaced while building these

Two genuine backend gaps showed up writing the workloads (recorded for later, not
blocking the harness):

1. **Polymorphic values in a generic container.** A `List<Shape>` holding mixed
   subtypes (`Circle`/`Square`/...) does not compile - the container element type
   lowers to the concrete newtype / bare trait, not `Rc<dyn …>`, so an upcast on
   `add` mismatches. `dispatch_poly.jux` works around it by selecting among
   base-typed locals instead of iterating a heterogeneous list. Polymorphism via
   base-typed locals itself works fine.
2. **Nullable class handle moves instead of share-cloning.** `Node? cur = head;`
   *moves* `head` (a non-nullable handle would share-clone), so a nullable
   reference can't be re-read in a later loop iteration. `graph_walk.jux` does a
   single traversal and relies on the runner repeating the *process* for a stable
   number.

## Future work
- **Reference implementations.** `optimizations.md`'s benchmark plan wants Jux vs
  Java (`-server`, warmed) vs C# (Release). Add equivalent programs under
  `benchmarks/ref/{java,csharp}/` and extend the runner to time them, once the
  comparison matters.
- **Memory (RSS)** and **allocation-throughput** numbers (peak working set) once a
  cross-platform measurement is wired into the runner.
