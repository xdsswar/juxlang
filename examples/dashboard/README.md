# Dashboard

A responsive operations dashboard, rendered from scratch in Jux.

There is no UI framework here and no drawing library. The whole thing is
integer arithmetic writing `0x00RRGGBB` pixels into a buffer: rounded panels,
filled line charts, bar tracks, gauge rings, and a 5x7 bitmap font that lives
in the source as plain numbers.

```
cd examples/dashboard

jux run                                        # live window, resizable
jux run -- --snapshot frame.ppm                # one frame to a file
jux run -- --snapshot frame.ppm --size 900x620
```

## Responsive

`Layout` is the only code that decides where anything goes. It reads the
surface size, picks a breakpoint, and hands each widget the rectangle it must
fit inside. No widget knows the window resized; they are simply asked to draw
somewhere else.

| Breakpoint | Width | Stat tiles | Charts | Side column |
|---|---|---|---|---|
| Wide | 1080+ | 4 across | 2 columns | yes |
| Medium | 720-1079 | 2 across | 1 column | yes, narrower |
| Compact | under 720 | stacked | 1 column | dropped |

Drag the window edge and the layout rearranges itself. The header names the
breakpoint it chose, so the change is visible rather than merely implied.

## Structure

The split is the point: `demo.dash` paints, and knows nothing about windows.
That is what lets the same frame go to a desktop window or to a file, and it is
why the rendering half needs no dependency at all.

| File | What it holds |
|---|---|
| `src/dash/theme.jux` | colors and spacing, as `const` |
| `src/dash/canvas.jux` | pixels, blending, rectangles, rounded corners, lines |
| `src/dash/font.jux` | a 5x7 bitmap font packed into integers, and text drawing |
| `src/dash/data.jux` | the metrics, from a seeded generator so a frame is reproducible |
| `src/dash/widgets.jux` | panel, stat tile, line chart, bar chart, gauge, event log |
| `src/dash/layout.jux` | breakpoints, and a rectangle per region |
| `src/dash/dashboard.jux` | one function that paints a whole frame |
| `src/main.jux` | the two front ends: a `minifb` window, and the snapshot |

## What it exercises

Packages and cross-package imports, classes with constructors that call their
own methods, records with methods, an enum with a method and a `switch` over
it, generics through `Vec<T>`, arrays as reference types, string interpolation,
`const` statics, bit manipulation on `u32`, ternaries, `for`-each over both
collections and `String.chars()`, and a foreign Rust crate driven in Jux
syntax.

It also found seven compiler bugs while it was being written, which is the
other reason a showcase this size is worth having.
