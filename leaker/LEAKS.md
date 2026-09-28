# Leaker: where the Jux abstraction over Rust leaks

This is a record of what happened when a Java/Kotlin-style developer wrote a
real desktop app (egui/eframe GUI plus invoice PDFs) in Jux, using the compiler
on branch `gaps` (`target/release/jux.exe`, built 2026-09-26). Each finding
has a minimal reproducer, the exact diagnostic, what I expected, the Jux-level
workaround (if any), and a severity:

- **blocker**: the API or feature cannot be used from Jux at all.
- **annoying**: there is a pure-Jux workaround, but you have to know Rust (or
  the generated code) to find it, or the code stops reading like Jux.
- **cosmetic**: it works, but Rust shows through.

"E0900" below always means `internal compiler error: the Rust generated for
this code does not compile`. The rustc text after it only shows up with
`jux --verbose build`. The program *type-checks* in every E0900 case, so the
failure is in lowering, not in the user's code.

## Where it stands now (2026-09-27, second round)

Gaps 30, 31 and 32 fixed L1 to L28, and the app was then **rewritten the way
a Java programmer would write it, with every workaround removed** (branch
`leaker-idiomatic`). The rewrite found ten more leaks, L29 to L39, listed
under "Round 2" below. Eight of them were fixed in the compiler on the same
branch, each with a test. The last two, L29 and L39, were fixed by gap 37:

- **L29** (fixed): egui's `Frame` is `rust.eframe.egui.Frame`, beside
  eframe's `rust.eframe.Frame`, and every egui signature takes the right one.
  The side navigation is dark again: `Panel.left(..).frame(new
  Frame().fill(Palette.NAV_BG).inner_margin(12.0f))`.
- **L39** (fixed): a crate's deprecated methods carry `@Deprecated` with the
  crate's note, and calling one is the Jux warning `W0491`.
- **L40** (blocker, found and fixed by gap 37): a static read held its lock
  to the end of the statement, so the app hung when the panel's lambda read a
  `Palette` color the same statement had read (see L29).

How the app is written now:

| Area | Result |
|---|---|
| egui via eframe | **Idiomatic.** It uses `Panel.left`/`Panel.bottom`/`Panel.right`, `CentralPanel`, `ScrollArea`, `Grid`, `ui.horizontal`/`vertical`/`group`, `ComboBox.show_ui`, `ui.button`, `ui.add(new Button(..).fill(..))`, `TextEdit.singleline(field)`, `new DragValue(line.quantity)` with a `custom_formatter`/`custom_parser`, `RichText`, `Color32` constants, and `selectable_value(filter, f, ..)` over a Jux enum. Lambdas change fields and captured locals directly. There are no `skip_ahead_auto_ids` lines, no `$"${x}"` copies, no values hoisted into locals for the compiler's sake, and no home-made toolkit. Wide windows put forms and totals in a side panel, and narrow ones put them below. |
| PDF via a Rust crate | **printpdf 0.12**, built-in Helvetica. `PdfCanvas` wraps a page's `Vec<Op>` with a top-left origin, and it measures text with the font's own glyph advances (`BuiltinFont.get_parsed_font()`). `InvoicePdf` does the layout: multi-page tables with repeated headers and "Page n of m" footers. printpdf was chosen over genpdf because genpdf needs TTF files on disk, embeds them (660 KB for one page), and its text comes out of `pypdf` letter-spaced. The PDFs pass `pypdf` strict parsing and render in PDFium. |
| Domain model / services | Unchanged. Only one construct is still not what Java writes: `n % list.len()` needs `(int)`, which is the spec's decision (L23). |

The first round's summary, for the record: egui worked only through
`run_ui_native` and a hand-made layout toolkit (L1, L3); all three PDF crates
were blocked (L19 to L21), so a PDF 1.4 writer was written in pure Jux; and
the domain model was clean apart from L15 and L22 to L24.

---

## GUI (egui 0.36 / eframe 0.36)

### L1. A Jux lambda passed to an egui container gets a *clone* of `&mut Ui`. **blocker**

Every egui container takes `impl FnOnce(&mut Ui) -> R`: panels, `CentralPanel`,
`ScrollArea`, `Grid`, `ui.horizontal`, `ui.vertical`, `ui.group`,
`ui.collapsing`, `ComboBox::show_ui`, `menu_button`, `Window::show`. Bindgen
marks all of these `@RustClosureRefs`, and the backend then starts the lambda
body with `let nav = nav.clone();`. `Ui` is not `Clone`, but it derefs to
`Context`, so the clone silently becomes a `Context`.

```jux
run_ui_native("Leaker", opts, (ui, frame) -> {
    Panel.left(ui.next_auto_id()).resizable(true).show_inside(ui, (nav) -> {
        nav.heading(new RichText("Leaker"));
    });
});
```
```
main.jux:13:5: [E0900] error: internal compiler error: the Rust generated for this code does not compile
error[E0599]: no method named `heading` found for struct `egui::Context` in the current scope
     nav.heading(egui::widget_text::RichText::new("Leaker".to_string()));
         ^^^^^^^ method not found in `egui::Context`
```
The same lowering captures outer locals by value (`let name = name.clone();`),
so `c.text_edit_singleline(name)` inside such a lambda would edit a copy even
if the rest compiled.

**Expected:** the lambda parameter is the `&mut Ui` egui passes in; writes
through it and through captured locals are visible afterwards.
**Workaround:** use only the top-level `eframe::run_ui_native(name, options, (ui, frame) -> ...)`,
whose closure is a free-function parameter with no `@RustClosureRefs`, and do
all layout by hand: `ui.new_child(new UiBuilder().max_rect(rect).layout(...))`
gives an owned child `Ui` for any rectangle. On top of that the app implements a
resizable split (`ui.interact(rect, id, Sense.drag())`), a scroll pane (offset
plus `set_clip_rect` plus wheel delta), and a table (see `src/leaker/ui/`).
Closures that receive a *copyable* value work fine, for example
`ComboBox.show_index(..., (i) -> WidgetText.Text(items[i]))` and
`ui.ctx().input((i) -> i.smooth_scroll_delta.y)`. In the second one the lambda
gets a clone of the whole `InputState` every frame (wasteful but correct).
**Decision:** I stayed on egui instead of switching crates. With the
workarounds, every screen that was asked for could be built.

**Status: fixed (gap 30, ERRATA E127).** A lambda in an egui container receives the `Ui` egui lends it, not a clone: `CentralPanel.show(ui, (panel) -> ...)`, `ScrollArea.vertical().show(...)`, `ui.horizontal(...)` and nested ones work, and the lambda can hand its `Ui` to Jux helper methods. Objects, collections and locals the lambda changes are the caller's own afterwards (`counter.bump()`, `names.push(..)`, `n++`). Keeping the lent `Ui` past the call (storing it in a field, returning it) is a Jux error, `E0454`, at the line that keeps it. Checked against egui 0.36 in a headless `Context.run_ui` probe and by `bin/juxc/tests/borrowed_foreign.rs`.

### L2. `impl Into<T>` parameters are surfaced as `T`, so a Jux `String` is rejected. **annoying**

`Ui::label(impl Into<WidgetText>)`, `heading(impl Into<RichText>)` and
`button(impl IntoAtoms)` show up as `label(WidgetText)` and so on.

```jux
ui.label("Hello");
ui.heading("Leaker");
```
```
[E0410] error: argument 1 to `label`: expected rust.egui.WidgetText, found String
[E0410] error: argument 1 to `heading`: expected rust.egui.RichText, found String
[E0410] error: argument 1 to `button`: expected rust.egui.IntoAtoms, found String
```
**Expected:** a String works wherever Rust would accept `&str`/`String` through `Into`.
**Workaround:** `ui.label(WidgetText.Text(s))`, `ui.heading(new RichText(s))`,
and `ui.colored_label(color, new RichText(s).size(20.0f))`. You have to read
the Rust source to learn which conversions exist.

**Status: fixed (gap 31, ERRATA E128).** `ui.label(s)` with a `String`, `ui.heading("Leaker")` and `ui.colored_label(c, "text")` compile as written. An `impl Into<T>` parameter is marked `@RustImpl T` and takes whatever `T`'s own `From` impls name (`@RustFrom`), the value passed as it is (probe `g31-leaker/probes/egui1`).

### L3. Passing a value to an interface-typed foreign parameter emits a non-existent path. **blocker** (for those APIs)

A foreign trait in parameter position (`IntoAtoms`, `Widget`, `AsIdSalt`,
`std::io::Write`) is lowered as `Box<dyn crate::rust::<crate>::Trait>`. That
module path does not exist, and the callee wants `impl Trait` anyway.

```jux
if (ui.button(new RichText("Click")).clicked()) { ... }
ui.add(new Button(new RichText("Click")));
var id = ui.id().with(ui.id());
doc.render(rust.std.File.create("x.pdf"));      // genpdf, W: Write
```
```
error[E0433]: cannot find `rust` in `crate`
   (Box::new(egui::widget_text::RichText::new("Click".to_string()))
        as Box<dyn crate::rust::eframe::IntoAtoms>),
                   ^^^^ could not find `rust` in the crate root
error[E0433]: cannot find `rust` in `crate`
   )) as Box<dyn crate::rust::eframe::Widget>),
error[E0433]: cannot find `rust` in `crate`
   .with((Box::new(ui.id()) as Box<dyn crate::rust::eframe::AsIdSalt>)),
error[E0433]: cannot find `rust` in `crate`
   (doc.render((Box::new(f) as Box<dyn crate::rust::std::Write>)))
```
**Expected:** `impl Trait` parameters accept any implementing value, as in Rust.
**Consequence:** `ui.button`, `ui.small_button`, `ui.selectable_label`,
`ui.add(...)` (so every `Widget`: `DragValue`, `Slider`, `Checkbox`, ...),
`ComboBox.from_id_salt`, `UiBuilder.id_salt` and `Id.with` are all unusable.
**Workaround:** buttons are built with the one constructor that takes concrete
types and are drawn with a method that takes `&mut Ui`:
`Button.opt_image_and_text(null, WidgetText.Text(text)).fill(c).atom_ui(u).response.clicked()`.
Ids come from `ui.next_auto_id()` plus `ui.skip_ahead_auto_ids(1)`. A combo box
id comes from its label (`ComboBox.from_label`), using distinct whitespace
labels (`" "`, `"  "`) so the label does not show.

**Status: fixed (gap 31, ERRATA E128).** `ui.button("Click")`, `ui.add(new Button(...))`, `ui.add(new DragValue(v))`, `id.with("child")` and genpdf's `render_to_file` compile and run. An `impl Trait` / `&dyn Trait` parameter takes the value as it is, and a foreign trait is always named by its real path, never `crate::rust::...` (probes `egui1`, `egui2`, `pdf_genpdf`).

### L4. Blanket-impl traits cannot be satisfied: `new Id("nav")`. **annoying**

egui has `impl<T: Hash + Debug> AsId for T`. The stub declares `interface AsId {}`
with no implementors, so nothing a Jux program has fits.
```jux
Panel.left(new Id("nav"));
```
```
[E0410] error: argument 1 to `rust.egui.Id`: expected rust.egui.AsId, found String
[E0413] error: no static method `new` on class `rust.egui.Id`
```
**Workaround:** `ui.next_auto_id()` (see L3).

**Status: fixed (gap 31, ERRATA E128).** `new Id("nav")` compiles: a blanket `impl<T: Hash + Debug> AsId for T` is recorded as `@RustBlanket("Hash + Debug")`, which a `String` meets (probe `egui1`).

### L5. One Rust type, two Jux types; and a mislabelled `Rect`/`Vec2`. **annoying**

eframe's stub merges egui into itself, so `egui::Ui` is both `rust.eframe.Ui`
and `rust.egui.Ui`, and the two do not unify. A bare interface name (`Widget`)
in a method signature resolved to the *other* stub's declaration:
```jux
// everything imported from rust.eframe; rust.egui is also a dependency
Button b = Button.opt_image_and_text(null, WidgetText.Text("x"));
b.ui(ui);                                  // Widget::ui
```
```
[E0410] error: argument 1 to `ui`: expected rust.egui.Ui, found rust.eframe.Ui
```
Separately, in `rust.egui`'s own stub, `Rect` and `Vec2` are kurbo's (from
accesskit): `@rust("egui::Rect") public class Rect { public double x0; ... }`,
`Vec2(double x, double y)` with `to_point()`. The eframe stub has the right
`emath::Rect`.
**Workaround:** import every egui type from `rust.eframe.*`, except `Color32`
(L6).

**Status: fixed (gap 31, ERRATA E128).** `rust.egui.Ui` and `rust.eframe.Ui` are one type: a crate bound in its own right is declared once and aliased by the stub that re-exports it, and a shared name means the definition the family publishes (`emath::Rect`, not kurbo's). A `Widget` imported from `rust.eframe` takes a `rust.egui.Ui` (probe `egui3`).

### L6. Re-exported crates must be listed by hand, and `Color32` is missing from eframe's stub. **annoying**

A program that uses only `rust.eframe` types still emits `use egui::...`,
`use emath::...` and `epaint::CornerRadius::same(...)`:
```
error[E0432]: unresolved import `egui`  --> use egui::Button;   help: use eframe::egui::Button;
error[E0433]: cannot find module or crate `epaint` in this scope
```
**Workaround:** list `rust.egui`, `rust.emath`, `rust.epaint` and `rust.ecolor`
in `jux.toml` as well (see the file). `Color32` is not declared in the eframe
stub at all, so it has to come from `rust.egui.Color32`. The eframe stub's
parameters that mention `Color32` are "unknown type" and accept it.

**Status: fixed (gap 31, ERRATA E128).** A program that lists only `rust.eframe` reaches `Color32`, `Rect`, `CornerRadius` and the rest; every path is written through `eframe` (`eframe::egui::Color32`), so `egui`, `emath`, `epaint` and `ecolor` need not be listed (probe `egui1`).

### L7. Removing a dependency leaves its stub active. **annoying**

After deleting `"rust.egui"` from `jux.toml`, `.jux-stubs/rust/egui.jux.d`
stays and keeps being loaded. The same `expected rust.egui.Ui` error came back
until I deleted the file by hand. The same happened with printpdf, lopdf and
genpdf stubs while trying crates (see L20 for how bad it can get).

**Status: fixed (gap 31, ERRATA E128).** Only the stubs of the dependencies `jux.toml` declares are loaded, and a generated stub whose dependency was removed is deleted (probe `egui3`, dropping `rust.egui`).

### L8. A field passed to a `&mut` parameter is lent as a temporary copy. **annoying**

```jux
class Form { public String name = "world";
    public void draw(Ui ui) { ui.text_edit_singleline(this.name); } }
```
```
error[E0596]: cannot borrow `__jux_arg0` as mutable, as it is not declared as mutable
   let __jux_arg0 = self.0.borrow().name.clone();
   ui.text_edit_singleline(&mut __jux_arg0)
```
Even if this compiled, the typing would go into the clone and be lost.
**Expected:** `&mut self.name` through the object's cell, the way a Java
programmer reads "pass the field".
**Workaround:** `String v = this.name; ui.text_edit_singleline(v); this.name = v;`
(`Gui.textField`). The same pattern is used for `ComboBox.show_index(u, sel, ...)`'s `&mut usize`.

**Status: fixed (gap 30, ERRATA E127).** `ui.text_edit_singleline(this.name)` edits the field itself. When another argument of the same call runs Jux code (a lambda, as in `ComboBox.show_index(ui, this.choice, ...)`), the field is copied in and written back right after the call, so the edit is never lost.

### L9. How a `Ui` parameter is passed depends on what the body happens to call. **annoying**

Borrow inference makes a foreign-typed parameter `&mut` only if the body calls
a `@MutSelf` method on it directly. Passing it on to another Jux method counts
as a *move*, and calling only `&self` methods also gives by-value:
```jux
public static void fill(Ui ui, Rect r, Color32 c) {
    ui.painter().rect_filled(r, CornerRadius.same((ubyte) 0), c);   // &self only
}
private void showNav(Ui ui, Rect nav) { Gui.row(ui, nav); }       // passes it on
```
```
69 |     pub fn fill(ui: egui::Ui, r: emath::Rect, color: egui::Color32) {
error[E0308]: mismatched types: expected `Ui`, found `&mut Ui`
   crate::leaker::ui::Gui::fill(ui, full.clone(), ...)
```
If the parameter *is* `&mut Ui`, handing it on to a foreign `&mut Ui`
parameter emits `&mut ui` on a non-`mut` binding:
```jux
public bool button(Ui ui, String text) {
    ui.add_space(0.0f);
    return Button.opt_image_and_text(null, WidgetText.Text(text)).atom_ui(ui).response.clicked();
}
```
```
error[E0596]: cannot borrow `ui` as mutable, as it is not declared as mutable
   b.atom_ui(&mut ui).response.clicked()
   note: the binding is already a mutable borrow
```
**Expected:** a parameter of a mutable object type is a borrow that can be
passed along. That is what "borrows vanish" (§G.3.4) promises.
**Workaround (in every method that takes a `Ui`):** start with a no-op
mutating call, `ui.skip_ahead_auto_ids((uint) 0);`, so the parameter becomes
`&mut Ui`. Before passing it to a foreign `&mut Ui` parameter, rebind it
(`var u = ui;`) and use only `u` from then on. This is purely ritual code, and
it appears 30+ times in `src/leaker/ui`.

**Status: fixed (gap 30, ERRATA E127).** A `Ui` parameter has one convention whatever the body calls: it is the caller's `Ui`, borrowed, and can be passed on to egui or to other Jux methods, directly or through a local (`var u = ui;`). Only a method that keeps it (stores it, returns it) owns it. In a copy of this app every `ui.skip_ahead_auto_ids((uint) 0);` line (45 of them) was deleted and the app still builds, runs every screen, and writes its PDFs.

### L10. Writing through a `@RustRefOut` accessor silently does nothing. **annoying (silent)**

```jux
ui.spacing_mut().text_edit_width = 120.0f;
```
lowers to
```rust
(u.spacing_mut()).clone().text_edit_width = width;
```
There is no diagnostic from either compiler, and the field write is lost. The
same applies to `style_mut()`, `visuals_mut()`, `get_object_mut()` and so on.
**Expected:** a write through `x_mut()` reaches the object, or a compile error.
**Workaround:** see L16.

**Status: fixed (gap 30, ERRATA E127).** `ui.spacing_mut().item_spacing = ...` and `var s = ui.spacing_mut(); s.indent = ...;` change the `Ui`'s spacing; reading it back with `ui.spacing()` shows the new value.

### L11. `WidgetText.RichText(...)` does not wrap in the `Arc` the stub erased. **annoying**

```jux
ui.label(WidgetText.RichText(new RichText("Leaker").size(20.0f)));
```
```
error[E0308]: mismatched types: expected `Arc<RichText>`, found `RichText`
```
**Workaround:** `ui.colored_label(color, new RichText(..))`, or
`WidgetText.Text(s).color(c)`.

**Status: fixed (gap 31, ERRATA E128).** `WidgetText.RichText(new RichText("big").size(20.0f))` compiles: the payload slot is `@RustArc RichText` and the argument is wrapped (probe `egui1`).

### L12. A lambda that captures a collection used elsewhere in the same call fails to borrow. **annoying**

```jux
ComboBox.from_label(WidgetText.Text("Customer"))
    .show_index(u, s, names.len(), (i) -> WidgetText.Text(names[i]));
```
```
error[E0505]: cannot move out of `names` because it is borrowed
   names.borrow().len(),        -- borrow of `names` occurs here
   move |i| WidgetText::Text(names.borrow()[(i) as usize].clone()),
```
**Workaround:** `uint count = (uint) names.len();` before the call.

**Status: fixed (gap 30, ERRATA E127).** `show_index(u, s, names.len(), (i) -> WidgetText.Text(names[i]))` compiles as written, for a local and for a field.

### L13. A foreign trait implemented for `String` is not known. **annoying**

```jux
TextEdit.singleline(v).desired_width(200.0f).show(u);     // v is a String
```
```
[E0410] error: argument 1 to `singleline`: expected rust.eframe.TextBuffer, found String
```
egui has `impl TextBuffer for String`, but bindgen only records impls whose
self type the crate itself declares. **Workaround:** `ui.text_edit_singleline(v)`
(generic `S`) plus a width trick (L16).

**Status: fixed (gap 31, ERRATA E128).** `TextEdit.singleline(n)` with a `String` compiles and edits `n`: `impl TextBuffer for String` is recorded on the trait as `@RustImplementedBy("String")` (probe `egui2`).

### L14. Argument hoisting moves `&mut Ui`. **annoying**

When any argument of a call reads through an object (a field, `inv.lines.len()`,
`inv.issueDate.iso()`), *all* arguments are hoisted into `let` temporaries. A
`&mut Ui` argument is then *moved* (`let __jux_arg0 = ui;`) instead of being
reborrowed (`&mut *ui`), and the next use of `ui` fails:
```jux
name = form.checkedField(ui, "Name", name, nameError);   // args read fields
email = form.checkedField(ui, "Email", email, emailError);
```
```
error[E0382]: use of moved value: `ui`
   let __jux_arg0 = ui;        -- value moved here
   ...
   let __jux_arg0 = ui;        ^^ value used here after move
```
The same happens for `this.table.show(ui, ...)` (receiver is a field).
**Workaround:** copy every field or computed argument into a local first
(`String n = name; name = form.checkedField(ui, "Name", n, ne);`), and call
methods of field-held objects through a local (`var t = table; t.show(ui, ...)`).

**Status: fixed (gap 30, ERRATA E127).** Arguments that read fields no longer move `ui`: `form.checkedField(ui, "Name", name, nameError)` twice in a row, and `this.table.show(ui, ...)`, work without copying anything into locals first.

### L16. `ui.style()` is an `Arc<Style>` in Rust but `Style` in Jux. **annoying**

```jux
var style = ui.style();
style.spacing.text_edit_width = width;
ui.set_style(style);
```
```
error[E0594]: cannot assign to data in an `Arc`
   style.spacing.text_edit_width = width;
   = help: trait `DerefMut` is required to modify through a dereference, but it is not implemented for `Arc<egui::Style>`
```
**Workaround:** build a fresh style and restate the visuals:
`var s = new Style(); s.visuals = Palette.visuals(); s.spacing.text_edit_width = w; ui.set_style(s);`
(`Gui.textFieldWidth`). Nested field writes on a *fresh* foreign value do work
(`v.widgets.inactive.bg_stroke = new Stroke(...)` in `Palette.visuals()`).

**Status: fixed (gap 31, ERRATA E128).** `var style = ui.style(); style.spacing.text_edit_width = 120.0f; ui.set_style(style);` compiles: `style()` is `@RustDerefOut` (the `Style` is copied out of the `Arc`) and `set_style` takes `impl Into<Arc<Style>>` (probe `egui2`).

### L17. Associated constants are not surfaced. **annoying**

`Color32::RED`, `Align2::LEFT_TOP`, `Id::NULL`, `Vec2::ZERO` and the rest do not
exist in the stubs, and `Align2` has no constructor, so
`Painter.text(pos, Align2, text, font, color)` cannot be called.
**Workaround:** `Color32.from_rgb(...)`, and text is drawn through child `Ui`s
with right-to-left layouts instead of the painter.

**Status: fixed (gap 31, ERRATA E128).** Associated constants are `static final` fields: `Color32.RED`, `Vec2.ZERO`, `Align2.LEFT_TOP` (probe `egui1`).

### L18. No numeric input widget. **annoying**

`DragValue(&mut Num value)`: `Num` is the method's generic parameter
(`new<Num: Numeric>(value: &mut Num)`) and appears unbound in the stub. It also
only renders through `ui.add` (L3) or `Widget.ui` (L5). `Slider` is the same.
**Workaround:** quantity is a text field between `-`/`+` buttons, and price,
discount and tax are text fields parsed by `Money.parse`/`Percent.parse`, with a
per-line "check input" state (`LineDraft`).

**Status: fixed (gap 31, ERRATA E128).** `ui.add(new DragValue(v).speed(0.1))` and `ui.add(new Slider(w, 0.0..=10.0))` compile over a `double` local: a foreign constructor's `&mut` slot lends the caller's place. Lending a FIELD in place is gap 30's rule and applies too (probe `egui2`).

### L25. A user class named like a foreign type is replaced by the foreign one in codegen. **annoying**

**Status: fixed (gap 32, ERRATA E124).** A bare name the unit resolves to the program's own class, record or interface is never re-read as a library enum of the same name. The leaker builds with the class named `Theme`; `examples/user_type_named_like_library_enum.jux` does the same against `std::cmp::Ordering`.

My `leaker.ui.Theme` (static consts plus static methods) type-checks, but every
*constant* access lowered to egui's enum:
```jux
if (navWidth < Theme.NAV_MIN) { ... }        // leaker.ui.Theme, same package
```
```
error[E0433]: cannot find `Theme` in `eframe`
   if self.0.borrow().navWidth < eframe::Theme::NAV_MIN {
   help: consider importing this struct through its public re-export: use crate::leaker::ui::Theme;
```
Static *method* calls on the same class lowered correctly. I had not imported
`eframe.Theme`. **Workaround:** renamed the class to `Palette`.

### L26. A parameter named `r` lowered as a collection handle. **annoying** (not minimized)

**Status: fixed and minimized (gap 32, ERRATA E124).** Root cause: constructor bodies emitted their locals into the backend's base name-to-type scope, which is never popped, so `int[] r` in `FontMetrics`'s constructor answered for `Rect r` in `Gui.fill` (a foreign-typed parameter registers no type of its own). Constructors now get their own scope and each top-level declaration starts from an empty one. `examples/local_names_do_not_leak.jux` is the minimized reproducer (an array local `r` in one constructor, `String r` parameters later).

In `Gui.jux`, `public static void fill(Ui ui, Rect r, Color32 c) { ui.painter().rect_filled(r, ...); }`
lowered to `r.borrow().clone()`:
```
error[E0599]: no method named `borrow` found for struct `egui::Rect` in the current scope
   r.borrow().clone(),
```
Renaming the parameter to `bounds` fixes it. Other files in the same program
have collection locals named `r` (`for (var r : rows)` and `var r = new Vec<String>()`).
A standalone probe with the same shape did *not* reproduce it, so the trigger
seems to be whole-program name-keyed state. It reproduces in this project by
renaming `bounds` back to `r` in `Gui.fill`.

---

## PDF

### L19. printpdf 0.12: tuple structs, struct-like enum variants and `Map` have no Jux spelling. **blocker**

```jux
var w = new Mm(210.0f);                                     // pub struct Mm(pub f32)
ops.push(Op.WriteTextBuiltinFont(BuiltinFont.Helvetica));   // Op::WriteTextBuiltinFont { items, font }
var doc = PdfDocument.from_html(html, new HashMap<String, Base64OrRaw>(), ...);
```
```
[E0411] error: `rust.printpdf.Mm` expects at most 0 arguments, got 1
[E0413] error: no static method `WriteTextBuiltinFont` on enum `rust.printpdf.Op`
[E0410] error: argument 2 to `from_html`: expected rust.std.Map<String, rust.printpdf.Base64OrRaw>, found rust.std.HashMap<String, rust.printpdf.Base64OrRaw>
[E0301] error: cannot find `Map` in this scope                 (for `new Map<String, Base64OrRaw>()`)
[E0410] ... expected rust.std.Map<...>, found rust.std.BTreeMap<...>
```
- A tuple struct (`Mm(pub f32)`, `Pt`, `Px`) is surfaced with only its
  `Default` constructor, so no length other than zero can be made. That rules
  out `PdfPage::new(Mm, Mm, ops)` and `Point::new(Mm, Mm)`.
- `Op` is surfaced as `enum Op { Marker, ..., SetFont, ShowText, ... }`: the
  struct-variant payloads are gone, so no drawing operation can be built.
- Bindgen maps `HashMap`/`BTreeMap` parameters to the canonical `Map<K,V>`, a
  type no Jux code can construct and that `HashMap`/`BTreeMap` do not convert
  to. So `from_html`, the only op-free entry point, is also unreachable.

**Status: fixed (gap 31, ERRATA E128).** A printpdf document with a filled rectangle and Helvetica text built from `new Mm(...)`, `Op.SetFillColor(...)`, `Op.DrawRectangle(...)`, `Op.SetFont(...)` and `Op.ShowText(...)` is written and parses in pypdf (probe `pdf_printpdf`); `PdfDocument.from_html(html, new BTreeMap<String, Base64OrRaw>(), ...)` runs (probe `pdf_html`). Tuple structs have their constructor and `_0`, named-field variants keep their fields, and maps keep their own names.

### L20. lopdf 0.45: a field named `operator` breaks the stub, and the error lands on `main`. **blocker**

`lopdf::content::Operation` has a public field `operator` (a Jux keyword). The
stub line `public String operator;` does not parse. Everything after it in the
1,739-line stub (lines 802 to 1739, including `Stream`, `StringFormat` and
`Object` helpers) is lost, and the resulting error is reported against the
*user's* `main`:
```
src\main.jux:1:8: [E0400] error: `main` is declared more than once at the top level
   secondary: .jux-stubs/rust/lopdf.jux.d:802..1739 "first declared here"
```
This happened even with `main() { print("x"); }` and no lopdf import, and
because of L7 it survived removing the dependency. It cost a lot of time: the
message points at the wrong file and the wrong problem.

**Status: fixed (gap 31, ERRATA E128).** lopdf's stub loads (`Document.load`, `get_pages`, `op.operator`, probe `pdf_lopdf`). A field named `operator` is read as a field; a stub that stops parsing as declarations is reported as `E0907` against the stub, never as a duplicate `main`.

### L21. genpdf 0.2: collection handles and `Path` resolution. **blocker**

genpdf's API is the most Java-like of the three (`Document.push`,
`TableLayout.row().element(...).push()`), and it type-checks. Lowering fails:
```jux
var t = new TableLayout(weights);                                   // Vec<uint>
doc.push(new Paragraph(new StyledString("Invoice", new Style().bold())));
doc.render_to_file(new Path("out.pdf"));
```
```
error[E0308]: mismatched types: expected `Vec<usize>`, found `Rc<JuxCell<Vec<usize>>>`
   genpdf::elements::TableLayout::new(weights);
error[E0277]: the trait bound `Style: From<Rc<__jux_rt::JuxCell<Style>>>` is not satisfied
   (Style is marked @RustCollection because it implements Extend, so every Style value is a shared handle)
[E0410] error: argument 1 to `render_to_file`: expected jux.std.io.Path, found rust.std.Path
[E0411] error: `jux.std.io.Path` expects at most 0 arguments, got 1       (for `new Path("fonts")` without an import)
```
- A Jux `Vec` passed to a by-value `Vec<usize>` constructor parameter is not
  unwrapped, although §G.6.6 says "a by-value foreign parameter takes the interior".
- `Style` (a value-like struct that implements `Extend<Effect>`) is classified
  as a collection and becomes a shared handle, which then does not satisfy
  `impl Into<Style>`.
- Inside the stub, `Path` resolves to `rust.std.Path` in `from_files(Path dir, ...)`
  but to Jux's own `jux.std.io.Path` helper class in `render_to_file(Path path)`.
  `render(Write)` hits L3.
- genpdf also needs TTF files on disk (`from_files("fonts", "Arial", Builtin.Helvetica)`).

**Decision:** the PDF is written by a small pure-Jux PDF 1.4 writer
(`PdfDocument`, `PdfPage`, `FontMetrics`, `InvoicePdf`): standard Helvetica and
Helvetica-Bold, AFM widths for right-aligned numbers, WinAnsi octal escapes,
multi-page tables with repeated headers and "Page n of m" footers, and a
correct xref table. It writes the file with `File.writeText` and creates the
folder with `rust.std.create_dir_all`. That last one worked first time: the
`Result` became an exception.

**Status: fixed (gap 31, ERRATA E128).** A genpdf document with a styled paragraph and a `TableLayout` built from a Jux `Vec<uint>` renders to a PDF (probe `pdf_genpdf`). `Style` is a value again (a collection must also iterate), a by-value `Vec` constructor parameter takes the interior, and `Path` in the stub is `std::path::Path`, which text also fills.

---

## Language / compiler, found along the way

### L15. A `String` passed to a call is moved, then reused. **annoying**

```jux
for (var word : text.split(" ")) {
    String candidate = line.length() == 0 ? word : line + " " + word;
    if (...) { line = word; }
}
// and
var color = k == statusColumn ? statusColor(text) : Palette.ink();
Gui.text(c, text, color, 13.5f);
```
```
error[E0382]: use of moved value: `word`
   word            -- value moved here
   line = word;    ^^^^ value used here after move
error[E0382]: use of moved value: `text`
   Table::statusColor(text)   -- value moved here
   Gui::text(&mut c, text, color, 13.5f32);   ^^^^ value used here after move
```
Strings are values in Jux. Using one twice is the most ordinary thing in Java.
**Workaround:** pass a fresh copy, `$"${word}"`.

**Status: fixed (gap 30, ERRATA E127).** A `String` read in one arm of `?:`, or passed to a method of the same class called by its bare name, is copied when it is read again later, so both reproducers above work without `$"${word}"`.

### L22. A parse error inside a class shows up as a bogus "class in function body" plus a duplicate `main`. **annoying**

**Status: fixed (gap 32, ERRATA E122).** The class keeps its other members and the one error is at the brace list: `an array initializer { ... } is only allowed where a variable or field is declared with its type ...; anywhere else write the array with its type: new int[] { 1, 2 }`. No `E0993`, no second `main`.

```jux
public class FontMetrics {
    private int[] regular;
    public FontMetrics() {
        regular = { 278, 278, 355 };          // array initializer in an assignment
    }
}
```
```
FontMetrics.jux:9:1: [E0993] error: a class cannot be declared inside a function body -- ...
main.jux:12:8: [E0400] error: `main` is declared more than once at the top level   (secondary span: FontMetrics.jux 9..49)
```
The real problem is that `{...}` is only allowed in a declaration
(`int[] r = {...}; regular = r;` works). Neither message says so.

### L23. `String.len()` and `Vec.len()` are Rust's `usize`. **cosmetic / annoying**

**Status: decided by the spec (ERRATA E123).** A Rust length stays a `uint` (§G.3.1, §K.12: a `Vec` has `len`, not `size`). `int n = v.len();`, `int last = v.len() - 1;`, `int at = a.len() + b.len();` and `i < v.len()` need no cast (§S.2.6, §S.2.7); where a length meets an `int` in one operator the `E0410` help now names the length and the two spellings that work. `String.length()` is the `int` count.

```jux
int at = header.len() + body.len();          // String
var p = store.products[n % store.products.len()];
```
```
[E0410] error: `int` and `uint` have no common type: no one integer type holds every value of both, ... cast one operand ...
```
Assigning `int n = v.len()` is accepted, but mixing it in arithmetic is not.
`String.length()` (an int) exists and is what I switched to, but `Vec` has no
`size()`. Counts end up with `(int)` casts.

### L24. A record's unqualified call to its own static method is not qualified in the output. **annoying**

**Status: fixed (gap 32, ERRATA E124).** A record's or an enum's own static method called bare is `Self::channel(...)`; an enum's methods are also in scope in its own bodies (they were `E0301`). `examples/own_static_calls.jux`.

```jux
public record Color(int r, int g, int b) {
    public String op() { String x = channel(r); return x; }
    private static String channel(int v) { return $"${v}"; }
}
```
```
error[E0425]: cannot find function `channel` in this scope
   let x: String = channel(self.r);
   help: consider using the associated function on `Self`: Self::channel(self.r)
```
The same code in a `class` works. **Workaround:** `Color.channel(r)`.

### L27. The E0900 report says nothing without `--verbose`. **cosmetic**

**Status: fixed (gap 32, ERRATA E125).** The `line`, `short` and `compact` formats append the rustc error to the E0900 line: `... does not compile (rustc reported error[E0599]: no method named ... ; a compiler bug, --verbose shows the full report)`. The `human` format already showed it as a note.

Every lowering failure above first appears as one line per statement, for
example `Gui.jux:44:16: [E0900] error: internal compiler error: the Rust generated for this code does not compile`
(or `... value used after it was moved`). The actual rustc error and the
generated Rust are only visible with `jux --verbose build`, and you then have
to read `target/.rust-build/bin-<name>/src/**.rs` to understand them. For a
user who is not supposed to know Rust, E0900 is where the abstraction ends.

### L28. Rust shows through in the everyday API. **cosmetic**

**Status: partly addressed (gap 32, ERRATA E126).** Stub generation no longer needs a nightly toolchain: without one, the default toolchain's rustdoc is run with `RUSTC_BOOTSTRAP=1` (same JSON, identical stubs). Hover, completion and signature help render Jux signatures with no `&`/`&mut`/`@MutSelf` (now pinned by a test); the markers are only in the `.jux.d` text. Verbatim snake_case names are the spec's decision (§G.4, no camelCase aliases). A literal that fits needs no `(ubyte)` cast; an `int` value does, as any narrowing (§S.2.7). The first-build cost is paid once per crate version (cached in `.jux-stubs/`).

- Foreign names are snake_case and verbatim (`text_edit_singleline`,
  `skip_ahead_auto_ids`, `rect_contains_pointer`). This is by design (§G.4),
  but it means reading egui's Rust docs.
- The stubs (what hover and completion show) say `&String`, `&mut Ui`,
  `@MutSelf`, `@RustRefOut`, `@RustClosureRefs("1")`, `uint`, `ubyte[4]`.
- `Color32.from_rgb((ubyte) 40, (ubyte) 44, (ubyte) 52)`: `u8` parameters need casts.
- Stub generation needs a nightly toolchain with `rustdoc` JSON. The first
  `jux check` after adding eframe took 72 s, and adding printpdf took 4 min.
- `print(x)` of a record works, but `Money.format()` had to be written by hand
  (no `String.format`/`%,.2f`). That is fair for a small std.

---

## What worked without friction (for balance)

- Records with methods, `operator+`/`operator-` and `operator string` (`Money`),
  records as values (`Date`), and enums with methods and exhaustive `switch`
  (`InvoiceStatus`, `Screen`).
- Nullable returns with `!!`, `try { s.parse<int>() } catch (Exception e)`,
  `String.split/trim/replace/starts_with/substring/chars`, string
  interpolation, and `+=` on strings.
- Collections are reference types. `Vec<Customer>` held in `Store` and read
  through `session.store.customers` alias correctly, and editing a `Customer`
  in place is visible in every invoice that points at it.
- The top-level `run_ui_native` lambda capturing a Jux object (`app.frame(ui)`)
  worked the first time.
- Builders on foreign types (`new ViewportBuilder().with_title(..).with_inner_size(..)`,
  `new UiBuilder().max_rect(r).layout(Layout.top_down(Align.Min))`), `@RustDefault`
  constructors (`new NativeOptions()`), `null` for `Option` parameters
  (`Button.opt_image_and_text(null, ...)`), assigning to foreign struct fields
  (`options.viewport = ...`, `v.widgets.inactive.bg_stroke = ...`), and
  `Result`-returning std functions turning into exceptions (`create_dir_all`).

## Workaround inventory

None is left. Before the second round, the app worked around L1 to L26 with:

- 45 `ui.skip_ahead_auto_ids((uint) 0)` lines;
- 4 `var u = ui;` rebinds;
- 3 `next_auto_id` id tricks;
- 11 locals standing in for field receivers;
- 23 field-to-local copies before calls;
- 3 `$"${x}"` String copies;
- the write-back helpers and `uint count` in `Gui`;
- 3 `Button.opt_image_and_text(..).atom_ui(..)` buttons;
- whitespace combo box labels;
- a home-made layout toolkit (`Gui`, `Form`, `Table`, `ScrollPane`: 488 lines);
- text fields parsed into numbers (`LineDraft`);
- a pure-Jux PDF writer (`PdfDocument`, `PdfPage`, `PdfColor`, `FontMetrics`: 299 lines).

The rewrite deleted all of it. The first round's code is in the history up to
`c16f909c`.

---

## Round 2: the idiomatic rewrite (2026-09-27)

These were found by writing the app with no workarounds, against the compiler
at `c16f909c` (gaps 30 to 32 closed). L30 to L38 are fixed on branch
`leaker-idiomatic`, and each fix has a test:

- `bin/juxc/tests/leaker_idioms.rs` builds and runs a program with every
  shape against a small crate that has egui's and printpdf's signatures,
  under the borrow self-check.
- `examples/field_initializer_calls.jux` covers L31 and L32 in pure Jux.

### L29. egui's `Frame` cannot be named. **annoying** (fixed, gap 37)

eframe declares its own `eframe::Frame` (the window's surroundings), and egui
declares `egui::Frame` (a panel's or group's fill, stroke and margins). The
eframe crate family has one Jux name per type, and the host's own item wins
(`FamilyPaths::rank`). So `egui::Frame` is dropped from the stub, and every
egui signature that takes one is read as eframe's:
```jux
import rust.eframe.Frame;
Panel.left(new Id("nav")).frame(new Frame().fill(Color32.DARK_GRAY)).show(ui, (nav) -> nav.label("x"));
```
```
[E0413] error: no method `fill` on type `rust.eframe.Frame`
```
`Panel.frame(Frame)`, `CentralPanel.frame(Frame)`, `TextEdit.frame(Frame)` and
`Ui.dnd_drop_zone(Frame, ..)` all name the wrong type. A stub cannot spell
two types called `Frame` from one family. A fix needs a second Jux name for
the losing type, such as a nested `rust.eframe.egui.Frame`, or a renamed
`EguiFrame` with a note on hover. Listing `rust.egui` as a dependency of its
own does not help, because the eframe signatures still name eframe's `Frame`.
**Workaround:** none. The app styled panels only through `Visuals`, so the
side navigation was no longer dark.

**Fix (gap 37, ERRATA E1XX-GAP37).** Not a `Frame` special case: when two
members of a crate family share a simple name, both keep a Jux name that
follows their Rust path. The family stub has a nested package for each module
the host publishes a member's items under, `rust.eframe.egui` for
`eframe::egui`, where egui's `Frame` is declared and every other egui type is
an alias of its one declaration. Every signature names the exact type:
`Panel.frame(rust.eframe.egui.Frame frame)`, and `run_ui_native`'s closure
takes `(Ui, rust.eframe.Frame)`. Importing both `Frame`s by their simple name
is `E0303`; `import ... as EguiFrame` names one beside the other. The app:
```jux
import rust.eframe.egui.Frame;
Panel.left(new Id("nav")).frame(new Frame().fill(Palette.NAV_BG).inner_margin(12.0f))...
```
Inside the panel the text and selection colors are set on `nav.visuals_mut()`.

**Found on the way (L40, fixed).** A first version built the panel's dark
`Visuals` in `Palette.navVisuals()`, which reads `Palette.NAV_BG`, from the
panel's lambda. The window never appeared: the process hung before eframe
painted its first frame. A `static final` computed at run time lives behind a
lock, and a read held the lock guard until the end of the STATEMENT, so the
`show(ui, (nav) -> ..)` lambda of the same statement waited for the guard of
`.frame(new Frame().fill(Palette.NAV_BG))` forever. A read now copies the value
out and releases the lock at once (`examples/static_read_in_chain.jux`).

### L30. A lambda argument lost its parameter type when the call's arguments were hoisted. **blocker** (fixed)

```jux
Panel.bottom(new Id("status")).show(ui, (bar) -> {
    bar.colored_label(color, session.status);      // the body reads a field
});
```
```
[E0900] ... (rustc reported error[E0282]: type annotations needed)
   let __jux_arg1 = { let __jux_this = self.clone(); move |bar| { ... bar.colored_label(..) } };
```
When any argument reads an object, every argument of the call is first bound
to a `let` (L14's machinery). A closure in a `let` has no expected type, so
rustc cannot learn that `bar` is the `&mut Ui` egui lends. This happened in
nearly every container call of the rewrite. **Fix:** a lambda is built at its
own slot when nothing after it is bound. Building a closure has no side
effect, so the evaluation order does not change.

### L31. A `static final` computed by a call became a Rust `const`. **annoying** (fixed)

```jux
private static final Color INK = PdfCanvas.rgb(30, 41, 59);   // printpdf's Color enum
```
```
[E0900] ... (rustc reported error[E0015]: cannot call non-const associated function `pdfcanvas::PdfCanvas::rgb` in constants)
```
Only fields of a Jux class or record type were initialized at run time; a
field typed as a crate's enum or a Jux enum became a `const`. **Fix:** an
initializer that is a call (or a `new`, or a `!!` or cast of a call) makes
the static lazily initialized, whatever its type. A `static final` number,
bool or String still has to fold at compile time (§T.11.1, `E0841`).

### L32. A field initializer was never type-checked, and `!!` in one did nothing. **annoying** (fixed)

```jux
private static final ParsedFont REGULAR = BuiltinFont.Helvetica.get_parsed_font()!!;
public String name = Lookup.find("a")!!;
```
```
[E0900] ... (rustc reported error[E0308]: mismatched types: expected `ParsedFont`, found `Option<ParsedFont>`)
```
Only lambda initializers were checked. Any other initializer was never
visited, so none of its types were recorded. The `!!` lowering asks for the
operand's type, and without it emitted the operand unchanged. **Fix:** every
field initializer is checked. A `static final String X = f()!!;` is now
reported as `E0841`, like any other call that does not fold.

### L33. A field lent to a widget builder was a copy. **blocker, silent** (fixed)

```jux
ui.add(TextEdit.singleline(name).desired_width(240.0f));   // name is a field
grid.add(new DragValue(line.quantity).range(1..=9999));      // a field of another object
```
```
[E0900] ... temporary dropped while in use (rustc reported error[E0716]: temporary value dropped while borrowed)
   let __jux_arg0 = TextEdit::singleline(&mut __jux_this.0.borrow().name.clone()).desired_width(240.0);
   cell.add(__jux_arg0)
```
Gap 30 lent a field in place to a `&mut` slot of an *instance* method. A
static crate function (`TextEdit.singleline`) and a crate constructor
(`new DragValue(..)`) still lent either a copy, `&mut x.borrow().name.clone()`,
or `&mut` through a shared `borrow()`. This went wrong in three ways:

- Once it compiled, the typing still went into the copy.
- The representation selector did not count these lends as writes, so it
  could give the class a representation with no cell
  (`E0900: the representation selector judged class ... never written`).
- Hoisting `ui.add(...)`'s argument into a `let` ended the widget's borrow
  before `add` ran.

**Fix:**

- Fields are lent through their owner's cell,
  `&mut __jux_this.0.borrow_mut().name`, on all three paths.
- Such a lend counts as a write of the class.
- An argument that builds a value holding a lent place stays at its slot.

### L34. A lent `Ui` was moved into an argument temporary. **blocker** (fixed)

```jux
ui.horizontal_wrapped((row) -> {
    tile(row, $"Overdue (${overdueCount})", overdue, Palette.DANGER);
    tile(row, "Drafts", draft, Palette.WARNING);
});
```
```
[E0900] ... (rustc reported error[E0382]: borrow of moved value: `row`)
   let __jux_arg0 = row;
```
This is L14 again, but for the `Ui` a lambda is lent rather than a parameter.
**Fix:** a borrowed foreign local is reborrowed into the temporary,
`&mut *row`.

### L35. A crate closure that takes `&str` and returns an `Option`. **annoying** (fixed)

```jux
new DragValue(cents).custom_parser((text) -> {
    var m = Money.parse(text);
    return m == null ? null : (double) m!!.cents;
});
```
```
error[E0308]: mismatched types: expected `String`, found `&str`      (Money.parse(text.clone()))
error[E0308]: mismatched types: expected `Option<f64>`, found `f64`  (if m.is_none() { None } else { .. as f64 })
```
There were two problems:

- egui lends `custom_parser` a `&str`. The lambda re-bound it with
  `.clone()`, so it stayed a `&str`.
- `c ? null : x` only wrapped `x` in `Some` when the context announced a
  nullable target. A lambda returning into a crate's `-> double?` slot did
  not announce one.

**Fix:** a `String` argument is owned with `.to_string()`, and a ternary
with a `null` arm whose type is nullable wraps its other arm.

### L36. A trailing `return` that lends a collection outlived it. **annoying** (fixed)

```jux
var warnings = new Vec<PdfWarnMsg>();
return doc.save(new PdfSaveOptions(), warnings);
```
```
[E0900] ... (rustc reported error[E0597]: `warnings` does not live long enough)
   crate::jux_arr(doc.save(&PdfSaveOptions::default(), &mut warnings.borrow_mut()))   // the block's tail
```
A block's tail expression keeps its temporaries past the block's locals
(Rust 2021). **Fix:** a trailing `return` whose call is handed a collection
local stays a `return ...;` statement. The same guard already covered field
reads.

### L37. The borrow self-check flagged a crate method named like a Jux one. **compiler-internal** (fixed)

Under `JUX_SELFCHECK=1`, a class with a method `save` failed on
`return doc.save(opts, warnings);`, a call to the crate's `save`. It was
reported as `E0900: the compiler would emit a borrow conflict here`
(`warnings is still borrowed mutably while save, which may use it, is handed
it as an argument`). The self-check recognizes Jux calls by name only.
**Fix:** an argument that is a guard's value (`&mut w.borrow_mut()`) is not
counted, since Jux passes handles and never a guard.

### L38. A method called on a local object passed a local `Ui` by value. **blocker** (fixed)

```jux
var ui = Ui.root();
var form = new Form();
form.show(ui);          // Form.show(Ui ui) takes it by `&mut`
```
```
error[E0308]: mismatched types: expected `&mut Ui`, found `Ui`
```
The test for L33 found this, not the app, because eframe always lends the
`Ui`. A one-name receiver (`form`) was looked up as a class name first, and
when that found nothing, the lookup gave up instead of trying the receiver's
type. **Fix:** a name that is neither a class nor a key in the by-reference
table is resolved as a receiver.

Fixing that uncovered the same shape in `main.jux`'s
`run_ui_native("Leaker", options, (ui, frame) -> app.frame(ui))`. The lambda's
`ui` had no type at all, because the closure type `(Ui, Frame)` was read in
the caller's imports, and `main.jux` imports neither `Ui` nor `Frame`. Once
`app.frame` was known to borrow its `Ui`, the call became `&mut ui` on a
parameter that is already a borrow (E0596). **Fix:** a free function's
closure type is read again in the function's own package when the caller's
imports leave it incomplete. The `Ui` that any crate closure is lent is
reborrowed (`&mut *ui`), as one lent by an egui container already was.

### L39. Deprecated crate methods are not marked. **cosmetic** (fixed, gap 37)

egui 0.36 renamed `Panel.show_inside` to `show`. The stub lists both with
nothing to tell them apart. `show_inside` compiles, and rustc's deprecation
warning shows only under `--verbose`. The app first used `show_inside`.
**Expected:** a stub carries `@Deprecated("Renamed to show")`, and the
checker warns at the call.

**Fix (gap 37).** The stub says `@Deprecated(message = "Renamed to `show`")
... show_inside<R>(..)`, and the call is
```
[W0491] warning: `Panel.show_inside` is deprecated: Renamed to `show`
```
The emitted crate allows rustc's `deprecated` lint, so rustc's own copy is
gone from `--verbose` too.

### Also noted (not leaks)

- `n % store.products.len()` still needs `(int)`: a Rust length is a `uint`
  (L23, decided by the spec).
- `static final String X = compute();` is `E0841` (§T.11.1: a `static final`
  String is a compile-time constant). A plain `static` works. A Java
  programmer would expect the first form to work too.
- printpdf's `Rect.from_xywh` has no paint mode, so a filled rectangle needs
  `rect.mode = PaintMode.Fill;`. That is printpdf's API.
