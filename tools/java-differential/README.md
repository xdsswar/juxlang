# Differential testing against Java

Thirty programs, each written twice: once in Jux, once in Java. Both run, and
the outputs must match line for line.

Java is the oracle, and that is the whole point. Every other test in this
repository pins an expectation that somebody wrote down, and a wrong
expectation is invisible: it passes, and the bug ships. Here nobody writes the
answer. The JDK produces it.

It found eleven bugs the first two times it ran, four of them silent (the
program compiled, ran, and printed something else):

- `a[j] = a[j + 1]` aborted at run time with "RefCell already borrowed" -- a
  bubble sort, the most ordinary thing an array is used for.
- A `String` inside a generic printed with quotes (`box("q")`).
- A generic class could not be another generic's type argument.
- `s?.length()` printed `Some(4)`; a null one printed `None`.
- `(short) 70000` did not compile, and `(int) 'z'` did not parse.
- `s.substring(7)` emitted `()` where the length belonged.

## Running it

```
bash tools/java-differential/run.sh          # every case
bash tools/java-differential/run.sh generic  # cases whose name contains "generic"
```

Needs `java` on PATH (17+; the single-file source launcher runs `Main.java`
with no `javac` step) and a built `jux` at `target/release`.

Results land in `results.txt` next to the script; emitted crates and compiler
diagnostics go under `emit/`.

## Writing a case

Two files:

- `cases/<name>.jux` -- the Jux program.
- `cases/<name>/Main.java` -- the same program in Java.

Keep them line-for-line equivalent, and keep the output DETERMINISTIC: no
identity addresses (Java prints `Main$Pair@78b66d36`, Jux prints
`Pair@0x195c4b1b0c0`, and neither repeats), no hash iteration order, no
timings. Sort before printing when order is not guaranteed.

Where Jux deliberately differs from Java, say so in the `.jux`:

```jux
// DIVERGES: <reason>
```

Those are reported separately rather than failing -- and if a declared
divergence ever stops happening, the runner says so too, because that is
equally worth knowing.

## What belongs here, and what does not

A case belongs here when Jux intends the same answer Java gives. That covers
most of the language: arithmetic, control flow, inheritance, generics,
exceptions, collections, casts.

It does not cover the places Jux is deliberately its own language --
reference-identity `===` on a value type, nullability written into the type,
`inf` rather than `Infinity`. Those are pinned by the ordinary example tests,
where the expectation is Jux's own.
