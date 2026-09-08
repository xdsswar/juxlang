# UI tests

One small program per diagnostic, with the compiler's exact output checked in
beside it.

This is the gate Rust leans on hardest, and the one Jux was missing. The
example corpus proves what the compiler **accepts**; these prove what it
**says when it refuses**. Before this existed, three assertions in the whole
suite looked at a diagnostic's text, so any of ~140 codes could change its
wording, move its span, or stop firing entirely with every test still green.

## Running

```
cargo test -p juxc --test ui                # check
JUX_BLESS=1 cargo test -p juxc --test ui    # re-bless every case
```

## Adding a case

1. Write `tests/ui/<name>.jux` — the smallest program that produces the
   message. One bad line, usually.
2. Run with `JUX_BLESS=1`.
3. **Read what it generated.** That reading is the point. A bad message
   blessed without thought is worse than no test, because it now looks like a
   decision somebody made.

Ask of each one: does it name the thing that is wrong, is the span on it, and
does it say what to do instead? The `String` cases are the model —
`no method 'equals' on 'String' -- '==' on a String is value equality` tells a
Java programmer the answer, not just the refusal.

## When a case fails

The output changed. Either a diagnostic got better, in which case re-bless and
commit the diff as the record of what users now see, or something regressed
and the diff says exactly how. Both are useful; neither should be re-blessed
without looking.

## What is checked beyond the text

- **No orphan `.expected`** — a file whose `.jux` was deleted stops testing
  anything while still looking like coverage.
- **No em-dash** — house rule for user-facing prose; the compiler should use
  `--`. This is checked over the blessed output rather than the source, so it
  covers every message at once.

## Related

`tools/no-ice-fuzz/` is the other half of the same idea: the compiler must
never PANIC, whatever the input. Rust's rule is that an internal compiler
error is always a bug however broken the source, and that harness breaks every
example mechanically to check it.
