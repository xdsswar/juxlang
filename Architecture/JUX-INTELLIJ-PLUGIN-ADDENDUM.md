# Jux Spec Addendum — IntelliJ Platform Plugin

**Status:** Proposed insertion. Specifies a native **IntelliJ Platform plugin** for Jux: the actions that surface `New → Jux File / Class / Interface / Enum / Record / Annotation` in the project view, the file templates that drive them, the file-type registration that makes `.jux` a first-class citizen, the path to "intelligent refactoring" (rename across files, move class, change signature, etc.), and the boundary between this plugin and the existing TextMate grammar (`JUX-EDITOR-TOOLING-ADDENDUM.md`) and language server (`JUX-LSP-SERVER-ADDENDUM.md`).

**Insertion points:**
- New §I.1 ("Goals and Non-Goals")
- New §I.2 ("Plugin Layout and Build")
- New §I.3 ("File Type Registration")
- New §I.4 ("Project Model — Source Roots and Package Inference")
- New §I.5 ("New-File Actions and Templates")
- New §I.6 ("LSP Integration")
- New §I.7 ("Refactoring Strategy")
- New §I.8 ("PSI: When and How Much")
- New §I.9 ("Distribution")
- New §I.10 ("Implementation Phases")
- New §I.11 ("Open Questions")

---

## Current State (2026-09-18)

> **Read this first.** The phased plan in §I.1 to §I.10 was written before
> the plugin existed. It proposed a TextMate-highlighted file type with no PSI,
> leaning on `juxc-lsp` for all semantics, and a declaration-level PSI only
> later. That is not what was built. The sections below are kept as design
> history; where they disagree with this section, this section describes the
> plugin as it is. The source of truth is
> `ide/intellij-plugin/src/main/resources/META-INF/plugin.xml`.

### What exists

- **A full PSI plugin.** `JuxParserDefinition` registers a recursive-descent
  parser written in Kotlin (`dev.jux.intellij.parser`: `JuxParser`,
  `JuxStatements`, `JuxExpressions`, `JuxTypes`) that builds a tree for
  declarations, statements and expressions. Highlighting is native
  (`JuxSyntaxHighlighterFactory` plus a color-settings page), not TextMate.
- **Tokens generated from the compiler's lexer.** The `generateJuxTokens`
  Gradle task builds `JuxTokenTypes` and `JuxKeywords` from
  `ide/intellij-plugin/grammar/jux-tokens.json`, which is exported from
  `juxc-lex` (`grammar_spec`) and `juxc-driver` (`grammar_export`). The
  plugin's token alphabet cannot drift from the compiler's. This answers the
  single-sourcing question §I.7 and §I.11 raised, at the token level; the
  grammar rules themselves are still hand-written in Kotlin.
- **A hybrid engine with `juxc-lsp`.** The plugin owns what lives in the PSI:
  completion, go-to declaration and implementation, find usages, rename,
  parameter info and the structure view. The language server supplies what
  needs the real type checker: diagnostics, hover with exact types, and code
  actions. `JuxLspDescriptor` switches the LSP client's own completion,
  go-to-definition, signature help, references and document symbols off so
  nothing appears twice. The server is reached through the IDE's native LSP
  client (`lsp.xml`, 2025.2+) or through the LSP4IJ fallback (`lsp4ij.xml`),
  never both. A status-bar widget names the engine that is serving.
- **Editing and navigation.** Structure view, folding, breadcrumbs, brace and
  quote handling, smart enter, move statement, surround-with, formatter with a
  code-style page, optimize imports, live and postfix templates, parameter
  info, parameter-name inlay hints, quick documentation, type hierarchy,
  go-to class and symbol backed by a file-based declaration index, and gutter
  markers for run, overrides, subtypes and observable properties.
- **Inspections.** Native inspections with quick-fixes (unreachable code,
  unused import, unused local, unresolved reference, missing `@Override`,
  observable-property checks, test-annotation placement, inheritance-shape
  checks), plus a missing-import annotator and an auto-importer.
- **Refactoring.** Rename (in place for locals and parameters, with a
  collision check), Introduce Variable, Introduce Constant, Inline Variable
  and Safe Delete. Generate constructor, getters, setters, and Implement /
  Override Methods.
- **Project, build, run and test.** New Project and New Module wizards,
  New Jux File (Class, Interface, Enum, Struct, Record, Annotation), New Jux
  Package, run configurations with a gutter run action, a test run
  configuration with a test-runner tree (`JuxTestEventsConverter`,
  `JuxTestConsoleProperties`, `JuxTestLocator`), Jux build and project tool
  windows, and IDE-wide toolchain settings that find `juxc`, `jux` and
  `juxc-lsp` from settings, `JUX_HOME`, `PATH` or common install locations.
  The toolchain's `.jux.d`
  stubs (standard library and bound Rust crates) are indexed as an external
  library.
- **Project model (§I.4).** Jux Sources Root and Jux Test Sources Root
  kinds with **Mark Directory as** actions, persisted through a JPS
  serializer; `JuxPackageResolver` as the one package-inference
  implementation (marked roots, then the nearest `jux.toml`, then a
  template-only fallback); a notification offering to mark `src/` and `test/`
  when a `jux.toml` project opens; the New Project wizard marks `src/` as a
  Jux Sources Root; the package-mismatch inspection with its three fixes; and
  Flatten Packages in the Project view for a Jux root.
- **Build.** IntelliJ Platform Gradle Plugin 2.x against
  `intellijIdea("2026.1.3")`, JDK 21 toolchain, `sinceBuild` 242, no
  `untilBuild` cap. Plugin id `dev.jux.lang`, version 0.0.9 at the time of
  writing.

Refactor | Move Class, Change Signature and Extract Method are built as
well, with the rest of Java's refactoring set (see `plugin.xml` and
`refactoring/`).

### Added in 0.0.8

- **Navigation.** Go-to, Find Usages and rename work for labels in
  `break`/`continue`, the subject of a smart cast (`x => Dog`), method and
  constructor references (`obj::m`, `Type::new`), names inside `int[N]`, and
  a sealed type's `permits` list. Pattern binders (`case C(var r)`,
  `case Dog d`, `x => Dog d`, `var Pt(x, _) = p`) are real locals: they
  resolve, rename, complete and have types.
- **Java's navigation actions.** Type Info (Ctrl+Shift+P), Go to Type
  Declaration (Ctrl+Shift+B), Go to Implementation and Quick Definition,
  File Structure with the Fields, Properties, Non-Public and Inherited
  toggles, and Go to Test / Create Test over the `src/` and `test/` layout.
- **Completion** after `A.super.`, `obj::` and `Type::`, and an enum's
  built-in helpers (`name`, `ordinal`, `fromName`, `cases`).
- **Folding** with Java's regions and defaults (imports and the file header
  folded, a doc comment shows its first sentence), with a Jux group under
  Settings | Editor | General | Code Folding.
- **Code style.** Wrapping and Braces and Blank Lines tabs, with Java's
  option names; the formatter honors every option shown, Force braces
  included. The defaults keep code as written.
- **Quick Documentation** in Java's layout: owner and signature, the
  inferred type of a `var`, `@param` / `@return` / `@throws` sections, and an
  override without a doc comment shows the one it overrides. It works for
  `.jux.d` stub members too.
- **New syntax parsed:** `@export { ... }` blocks, generators, `operator..`
  and `operator..=`, bodiless interface operators, or-patterns and nested
  destructuring. The corpus sweeps (parse, highlighting, completion,
  inspections) run over `examples/` and every `tests/rux-lessons` program.

### Added in 0.0.9

- **Operators as symbols.** A use such as `a * 2.0`, `start..end` or
  `price += 1L` resolves to the operator the compiler calls: the left
  operand's type and its supertypes supply the candidates, and the right
  operand's type picks among operators with the same symbol, the way a call
  picks a method overload (Operators addendum §O.2.3). Ctrl+B on the operator
  token navigates there, the expression's type is that operator's return
  type, and Find Usages on an operator declaration lists its uses (an
  operator has no name to index, so a dedicated references search walks the
  Jux files that contain the symbol).
- **Interface operators.** Gutter markers link an interface's
  `T operator+(T other);` and the class operators that fill it, both ways.
  An inspection mirrors E0936 (a body, more than one operand, or an operator
  other than `+ - * / % & | ^ << >>` in an interface; a bodiless operator
  elsewhere), with a fix that removes the body.
- **Generators.** `yield` completes only inside a named function that
  returns `Iterator<T>` (`Stream<T>` when `async`). An inspection mirrors
  E0990, E0994, E0995, E0996 and E0997, with fixes that change the return
  type and turn `return value;` into `yield value;`.
- **Java parity, from an audit of IntelliJ's Java support.**
  - Completion in a class body offers the inherited methods to implement or
    override; accepting one writes `@override` and the whole member.
  - Intention "Create missing 'case' branches" for a `switch` over an enum
    or a sealed type.
  - Inspection "'if' statement can be simplified" (a branch that only
    returns or assigns `true`/`false`).
  - Inspection "Indexed loop can be a for-each" (`for (int i = 0; i <
    xs.len(); i++)` or `for (var i : 0..xs.len())` that only reads `xs[i]`).

### Known gaps against the Java plugin

The audit also listed what is not built yet, most valuable first: dataflow
checks (a value assigned and never read, a condition always true on one
path), "Field may be final", Pull Members Up / Push Members Down and
Extract Interface / Superclass, chain completion (`.` after a call offering
what the chain can reach), and postfix templates for the newer syntax
(`.yield`, `.switch` with generated arms).

### Still open

- **Per-platform distributions with a bundled `juxc-lsp`.** The build can
  bundle one binary (§I.9); publishing one plugin build per platform is not
  set up.
- **Marketplace publishing** is configured (§I.9) but needs the publisher
  account and its secrets.
- A debugger (out of scope, as before).

---

## Design Philosophy (Non-Normative)

> **History.** This section and the phase plan below describe the original
> proposal. See "Current State" above for what the plugin actually is.

The TextMate grammar gives every editor a coloring story. The LSP gives every editor diagnostics, hover, goto-def, and rename. Both work in IntelliJ today. But the IntelliJ user experience is gated on three things those layers cannot deliver:

1. **`New → Jux <thing>` menus** — driven by the platform's `NewFileAction` infrastructure and per-file-type templates. TextMate cannot add a menu item; LSP cannot create files.
2. **Native refactoring UX** — IntelliJ's rename dialog, "Find Usages" tool window, and "Refactor This" popup are bound to the platform's **PSI** (Program Structure Interface). LSP refactoring works but appears in a different UI surface (LSP4IJ exposes it as a command, not as a first-class Refactor menu entry).
3. **File-type-aware project view** — icons, `*.jux` association without manual configuration, "Mark Directory As → Sources Root" treating Jux modules correctly.

This addendum specifies a plugin that delivers items (1) and (3) cheaply, and a **staged** path to (2). The expensive part — full PSI — is deferred until measured demand justifies it. In the meantime, semantic features come from `juxc-lsp` and the plugin acts as a launcher + UX-bridge.

### What this plugin is NOT

*(Superseded in part: the plugin now does carry its own Kotlin parser, with
its tokens generated from `juxc-lex`. See "Current State".)*

- It is NOT a Kotlin re-implementation of the Jux parser. The Rust front end is canonical; duplicating it in Java/Kotlin would invite drift and double maintenance cost.
- It is NOT a debugger. Debugging belongs to a future runtime-side addendum.
- It is NOT exclusively for IntelliJ IDEA. The same plugin works in CLion, GoLand, PyCharm, WebStorm, Rider, RustRover, and Android Studio (any IDE built on the IntelliJ Platform). The plugin descriptor declares `com.intellij.modules.platform` only — no IDE-specific module dependencies.

---

## §I.1 — Goals and Non-Goals

> **History.** The phase numbers in this table are the original plan. Rows
> 1, 4, 8 to 18 are built (row 2 was replaced by native highlighting, and
> rows 15 and 16 are served by the plugin's PSI rather than by LSP
> delegation). Rows 3, 5, 6, 7, 19, 20, 21 and 22 are still open. The
> Non-Goals list is also out of date: the PSI covers expressions and
> statements, and Introduce Variable, Inline Variable, live templates,
> postfix templates and inspections all ship.

### Goals

| # | Feature                                                                            | Phase |
|---|------------------------------------------------------------------------------------|-------|
| 1 | `*.jux` registered as a real file type with an icon                                | 1     |
| 2 | Bundled TextMate grammar (`jux.tmbundle`) loaded automatically                     | 1     |
| 3 | Source roots: directories markable as "Jux Sources Root" / "Jux Test Sources Root" / "Resources Root" | 1 |
| 4 | Package inference from directory: new-file template's `${PACKAGE}` resolves from source-root-relative path | 1 |
| 5 | Package mismatch inspection: warn when in-file `package` declaration disagrees with on-disk location, with a quick-fix that rewrites the `package` line OR moves the file | 2 |
| 6 | "Mark Directory as → Jux Sources Root / Test Sources Root / Resources Root" actions in the project view | 1 |
| 7 | Project View "Package View" mode: collapse nested single-child packages into `com.example.foo` style entries (Java-like) | 2 |
| 8 | `New → Jux File` (empty file with auto-inferred `package` line)                                  | 1     |
| 9 | `New → Jux Class`                                                                  | 1     |
| 10 | `New → Jux Interface`                                                              | 1     |
| 11 | `New → Jux Enum`                                                                   | 1     |
| 12 | `New → Jux Record`                                                                 | 1     |
| 13 | `New → Jux Annotation`                                                             | 1     |
| 14 | Auto-launch and manage `juxc-lsp` per project                                      | 2     |
| 15 | Refactor → Rename for identifiers (delegates to LSP `textDocument/rename`)         | 2     |
| 16 | Find Usages (delegates to LSP `textDocument/references`)                           | 2     |
| 17 | Auto-import: typing an unresolved type surfaces a quick-fix to add `import com.example.Foo;` | 2 |
| 18 | Native PSI for class/interface/enum/record/annotation top-level declarations only  | 3     |
| 19 | Refactor → Move Class (PSI-backed; physically moves the file, rewrites `package` + project-wide imports) | 3 |
| 20 | Refactor → Change Signature (function-level)                                       | 4     |
| 21 | Refactor → Extract Method                                                          | 4     |
| 22 | Structural Search and Replace (IntelliJ SSR) for Jux                               | 5     |

### Non-Goals

- Full PSI tree for expressions and statements. The plugin's PSI is **declaration-level only** — class, interface, enum, record, annotation, method signature, field declaration. Bodies are opaque to the PSI; expression-level refactoring (extract variable, inline) is out of scope until/unless demand justifies the parser duplication.
- Run/Debug configurations beyond a thin "Run `juxc` on this file" action. The build system addendum (`JUX-BUILD-SYSTEM-ADDENDUM.md`) owns the project model.
- Live Templates, postfix completions, inspections-as-code. Reserved for phase 5+.

---

## §I.2 — Plugin Layout and Build

### Repository location

*(History: the planned layout. The real tree has no `textmate/` resource
folder and adds `parser/`, `psi/`, `resolve/`, `completion/`,
`inspections/`, `refactoring/`, `run/`, `lsp4ij/` and more; the descriptor
snippet below also predates the plugin id change to `dev.jux.lang`.)*

```
ide/intellij-plugin/
├── build.gradle.kts
├── settings.gradle.kts
├── gradle.properties
├── gradle/wrapper/
└── src/
    └── main/
        ├── kotlin/
        │   └── dev/jux/intellij/
        │       ├── JuxFileType.kt
        │       ├── JuxIcons.kt
        │       ├── JuxLanguage.kt
        │       ├── actions/
        │       │   ├── NewJuxFileAction.kt
        │       │   ├── NewJuxClassAction.kt
        │       │   ├── NewJuxInterfaceAction.kt
        │       │   ├── NewJuxEnumAction.kt
        │       │   ├── NewJuxRecordAction.kt
        │       │   └── NewJuxAnnotationAction.kt
        │       ├── lsp/
        │       │   ├── JuxLspServerSupportProvider.kt   # Ultimate-only path
        │       │   └── JuxLspDescriptor.kt
        │       ├── psi/                                  # phase 3+
        │       └── refactor/                             # phase 3+
        └── resources/
            ├── META-INF/
            │   ├── plugin.xml
            │   └── pluginIcon.svg
            ├── icons/
            │   └── jux.svg                               # file-type icon
            ├── fileTemplates/
            │   └── internal/
            │       ├── Jux File.jux.ft
            │       ├── Jux Class.jux.ft
            │       ├── Jux Interface.jux.ft
            │       ├── Jux Enum.jux.ft
            │       ├── Jux Record.jux.ft
            │       └── Jux Annotation.jux.ft
            └── textmate/
                └── jux.tmbundle/                         # copied from editors/
                    ├── info.plist
                    └── Syntaxes/
                        └── jux.tmLanguage.json
```

### Build tooling

The plugin uses the **IntelliJ Platform Gradle Plugin** (`org.jetbrains.intellij.platform`, 2.x) — the modern successor to the legacy `gradle-intellij-plugin`. The build is Kotlin-DSL-only; no Groovy.

### Plugin descriptor (`plugin.xml`) skeleton

```xml
<idea-plugin>
  <id>dev.jux.intellij</id>
  <name>Jux Language</name>
  <vendor email="contact@xdsswar.dev" url="https://github.com/xdsswar/juxlang">
    XTREME SOFTWARE SOLUTIONS
  </vendor>

  <depends>com.intellij.modules.platform</depends>
  <depends optional="true" config-file="lsp.xml">com.intellij.modules.lsp</depends>

  <extensions defaultExtensionNs="com.intellij">
    <fileType name="Jux File"
              implementationClass="dev.jux.intellij.JuxFileType"
              fieldName="INSTANCE"
              language="Jux"
              extensions="jux"/>
    <internalFileTemplate name="Jux File"/>
    <internalFileTemplate name="Jux Class"/>
    <internalFileTemplate name="Jux Interface"/>
    <internalFileTemplate name="Jux Enum"/>
    <internalFileTemplate name="Jux Record"/>
    <internalFileTemplate name="Jux Annotation"/>
  </extensions>

  <actions>
    <group id="Jux.NewGroup" class="dev.jux.intellij.actions.NewJuxGroup"
           popup="true" text="Jux" icon="dev.jux.intellij.JuxIcons.FILE">
      <add-to-group group-id="NewGroup" anchor="before" relative-to-action="NewFile"/>
      <action id="Jux.NewFile"        class="dev.jux.intellij.actions.NewJuxFileAction"/>
      <action id="Jux.NewClass"       class="dev.jux.intellij.actions.NewJuxClassAction"/>
      <action id="Jux.NewInterface"   class="dev.jux.intellij.actions.NewJuxInterfaceAction"/>
      <action id="Jux.NewEnum"        class="dev.jux.intellij.actions.NewJuxEnumAction"/>
      <action id="Jux.NewRecord"      class="dev.jux.intellij.actions.NewJuxRecordAction"/>
      <action id="Jux.NewAnnotation"  class="dev.jux.intellij.actions.NewJuxAnnotationAction"/>
    </group>
  </actions>
</idea-plugin>
```

### Compatibility target

Minimum platform version: **2024.2** (sinceBuild `242`, `pluginSinceBuild` in `gradle.properties`). It is the first IDE on JBR 21, which the plugin's Java 21 bytecode requires; 2024.1 runs on JBR 17 and cannot load it. There is no `untilBuild` cap. The plugin compiles against the latest stable release at the time of each marketplace push (`platformVersion`). The plugin id is `dev.jux.lang`, not the `dev.jux.intellij` of the skeleton above.

---

## §I.3 — File Type Registration

`JuxFileType` extends `com.intellij.openapi.fileTypes.LanguageFileType`. The minimal skeleton:

```kotlin
object JuxFileType : LanguageFileType(JuxLanguage) {
    override fun getName(): String = "Jux File"
    override fun getDescription(): String = "Jux source file"
    override fun getDefaultExtension(): String = "jux"
    override fun getIcon(): Icon = JuxIcons.FILE
}
```

*(History: the plugin now registers `JuxParserDefinition` and a native
syntax highlighter; the TextMate path described in the next two paragraphs
was never shipped.)*

`JuxLanguage` extends `com.intellij.lang.Language` with the canonical id `"Jux"`. **No `ParserDefinition` is registered in Phase 1** — the language is "syntax-highlight-only" until the PSI work in §I.8 lands. This is a supported configuration (Markdown, plain-text-with-syntax-highlighting, and several JetBrains-bundled languages do exactly this).

Highlighting is provided by the bundled TextMate grammar, loaded from `resources/textmate/jux.tmbundle/` at plugin startup via the platform's `TextMateBundleProvider` extension point.

### Icon

A new `icons/jux.svg` is shipped with the plugin. SVG, square viewbox, monochrome (the platform recolors based on theme). The file-type icon, the action-group icon, and the plugin marketplace icon are three different assets; only the file-type icon is normative here.

---

## §I.4 — Project Model — Source Roots and Package Inference

The plugin treats `.jux` files exactly the way the IntelliJ Java plugin treats `.java` files: package identity is **derived from a file's path relative to the nearest source root**, not from the `package` declaration written inside the file. The declaration and the directory MUST agree; when they don't, the IDE surfaces an inspection with two quick-fixes (rewrite the `package` line; move the file).

### Source-root kinds

The plugin registers two source-root kinds of its own (`JuxSourceRootType`) and reuses the platform's resources root. Each has a `ModuleSourceRootEditHandler` (the `projectStructure.sourceRootEditHandler` extension point) for its folder icon and Project Structure presentation, and a JPS serializer (`JuxJpsModelSerializerExtension`, loaded through `<jps.plugin/>` and `META-INF/services`) so a marked root is saved in the module file under the ids `jux-source` and `jux-test-source`:

| Kind                       | Icon overlay | Purpose                                                              |
|----------------------------|--------------|----------------------------------------------------------------------|
| **Jux Sources Root**       | blue folder  | Production `.jux` source — the package root for application code     |
| **Jux Test Sources Root**  | green folder | Test `.jux` source — same package semantics, separate classpath bucket for the build |
| **Resources Root**         | reused from platform | Non-source data files bundled with the module                   |

The user marks a directory by right-clicking in the Project View → **Mark Directory as → Jux Sources Root** (or Jux Test Sources Root). The actions are `MarkSourceRootAction` subclasses in the platform's `MarkRootGroup`, the submenu Java and Kotlin roots use. The New Project wizard marks the `src/` it creates as a Jux Sources Root.

Auto-detection: when the plugin opens a project containing a `jux.toml` whose `src/` or `test/` is not yet a Jux root, it offers to mark `src/` as Jux Sources Root and `test/` as Jux Test Sources Root via a non-modal notification (per the layout in `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.1). The user can accept, choose **Don't ask again** for the project, or configure manually. Marking is optional: the manifest rule below already infers every package correctly, and the marks add the folder colors, test scoping, and the roots the language server is told about.

### Package inference algorithm

Given a `.jux` file at absolute path `P`, the inferred package name is computed as follows:

1. Walk upward from `P` to find the nearest directory `R` marked as **Jux Sources Root** or **Jux Test Sources Root**. If none exists, use the nearest `jux.toml` (see "Defaults when no source root is marked"). If there is none either, the file has no package root and `${PACKAGE}` resolves to the empty string.
2. Let `rel` be the path of `P`'s parent directory relative to `R`.
3. If `rel` is empty (the file sits directly inside the source root), `${PACKAGE}` is the empty string. The template MUST omit the `package` line in that case.
4. Otherwise, replace path separators (`/` or `\`) with `.` to produce the dotted package name. Example: `src/com/example/foo/Bar.jux` under source root `src/` → `${PACKAGE} = "com.example.foo"`.

This algorithm is implemented in a single utility, `JuxPackageResolver` (`inferPackage`, `rootFor`, `expectedPackages`), and is the canonical source for:

- The `${PACKAGE}` template variable in §I.5.
- The package-mismatch inspection (below).
- The Move Class refactoring's target-package computation (§I.7 tier 2).
- The Project View's flattened packages (below).
- The `jux.sourceRoots` list sent to the LSP.

### Package-mismatch inspection

A `LocalInspectionTool` named `JuxPackageMismatchInspection` compares the `package` declaration at the top of an opened file against the package its location implies. It checks only a location the Jux layout vouches for: a marked Jux root, or the `src/` and `test/` of a `jux.toml` project. A file under neither takes its package from its declaration, exactly as `juxc` treats a file outside a `src/` root (§B.1.1), and is never flagged. In a test root the production package plus `.test` is accepted too, the §B.1.2 convention. On mismatch, the editor underlines the package name with a warning:

> Package name `com.wrong.pkg` does not correspond to the file location. Expected `com.example.foo`.

Two quick-fixes attach:

1. **Set package name to `com.example.foo`**, which edits the `package` line in place (or removes it for a file directly in the root, which must have none).
2. **Move file to `com/wrong/pkg/`**, which moves the file to the directory its declared package names, under the same root. The declared package does not change, so no import anywhere needs rewriting. The fix is not offered when a file of the same name is already there.

If the file has no `package` declaration at all but its location implies one, the inspection offers a single quick-fix: **Add `package com.example.foo;`**.

### Project View — Package View mode

Two Project View options give Jux the Java plugin's package view. **Compact Middle Packages** (the platform's compact-directories option) collapses a single-child chain of directories `com/example/foo/` into one node for any directory, so it applies to Jux with no extra code. **Flatten Packages** relies on Java's package model, so `JuxPackageTreeStructureProvider` supplies it for a Jux root: with the option on, the root lists one node per package that holds files, labelled with the dotted name (`com.example.foo`) and listing that package's files. Packages with no files are left out, as Java does, unless **Hide Empty Middle Packages** is off.

### Coordination with `juxc-lsp`

The LSP server learns about source roots via a custom `workspace/configuration` request keyed `jux.sourceRoots`. The value is a list of `{ "path": <absolute path>, "kind": "sources" | "tests", "origin": "marked" | "manifest" }`, one per package root: every marked Jux root, and for each `jux.toml` its `src/` (or its own directory when it has no `src/`) and `test/`. The plugin answers the request with that list, passes the same list at start-up as the initialization option `{ "jux": { "sourceRoots": [...] } }`, and sends `workspace/didChangeConfiguration` with it whenever roots are marked or unmarked. Both clients do this: the native LSP client through `JuxLspDescriptor`, LSP4IJ through `JuxLsp4ijLanguageClient`. **Pending on the server:** `juxc-lsp` does not read the list yet. The protocol-level definition of this custom message is OUT of scope for the LSP addendum (`JUX-LSP-SERVER-ADDENDUM.md`) and IS in scope for this plugin's bring-up; if a second editor needs the same configuration channel, it gets promoted to the LSP addendum.

### Defaults when no source root is marked

If the user never marks a source root, the plugin falls back to **the project's `jux.toml` directory as the implicit source root** when a `jux.toml` is present, walking down to `src/` if that subdirectory exists. This makes "open the project, hit New → Jux Class, type `Foo`" Just Work for the common case without any manual configuration. Power users with non-standard layouts still need to mark roots explicitly.

---

## §I.5 — New-File Actions and Templates

### Template files

Each template lives in `resources/fileTemplates/internal/` and uses IntelliJ's Apache-Velocity-derived template language. The `.ft` extension marks the file as a template; the prefix before `.ft` is the suggested filename pattern. Every template begins with `#parse("Jux File Header.jux")`, a license-comment include from `resources/fileTemplates/includes/` that the user can edit under **Settings | Editor | File and Code Templates**; the listings below omit that line. There is also a `Jux Struct.jux.ft` template (`public struct ${NAME} { }`), and the New Jux File dialog offers all seven kinds.

#### `Jux File.jux.ft`

```
#if (${PACKAGE} && ${PACKAGE} != "")package ${PACKAGE};

#end
```

Plain file with an optional `package` declaration inferred from the target directory.

#### `Jux Class.jux.ft`

```
#if (${PACKAGE} && ${PACKAGE} != "")package ${PACKAGE};

#end
public class ${NAME} {
}
```

#### `Jux Interface.jux.ft`

```
#if (${PACKAGE} && ${PACKAGE} != "")package ${PACKAGE};

#end
public interface ${NAME} {
}
```

#### `Jux Enum.jux.ft`

```
#if (${PACKAGE} && ${PACKAGE} != "")package ${PACKAGE};

#end
public enum ${NAME} {
}
```

Per the saved feedback `feedback_enum_syntax.md`: variants are NOT prefixed with `case`. The template body is intentionally empty so the user fills in variants Java/Rust-style.

#### `Jux Record.jux.ft`

```
#if (${PACKAGE} && ${PACKAGE} != "")package ${PACKAGE};

#end
public record ${NAME}() {
}
```

#### `Jux Annotation.jux.ft`

```
#if (${PACKAGE} && ${PACKAGE} != "")package ${PACKAGE};

#end
public annotation ${NAME} {
}
```

### Action implementation

Each `New<Thing>Action` extends `com.intellij.ide.actions.CreateFileFromTemplateAction`. The action overrides:

- `buildDialog(project, directory, builder)` — adds the template names to the new-file dialog.
- `getActionName(directory, newName, templateName)` — used for the undo-history label.

The `${PACKAGE}` template variable is resolved by `JuxPackageResolver.inferPackage(targetDirectory)` per the algorithm in §I.4. If the file is created outside any source root, `${PACKAGE}` resolves to the empty string and the `#if` block in the template suppresses the `package` line. The `${NAME}` variable is the user-typed name from the New-file dialog and MUST be a valid Jux identifier; the action validates this before instantiating the template.

### Where the New menu appears

Per the `plugin.xml` snippet in §I.2: a `Jux` submenu is anchored **before** the standard `New File` entry inside `NewGroup`. Submenu contents are the six Jux-specific actions plus a separator. Right-clicking any directory in the Project view exposes the menu; the Cmd/Ctrl+N "New" popup in the editor also includes it.

### Menu availability

The Jux submenu is **always visible** under any directory inside an IntelliJ project — there's no requirement that the target be inside a Jux source root. Creating a Jux file outside a source root is legal; the template just omits the `package` line. (Java behaves the same way.)

---

## §I.6 — LSP Integration

> **History.** The shipped integration differs from this sketch in three
> ways: `juxc-lsp` is resolved through the toolchain settings, `JUX_HOME`,
> `PATH` and common install locations (`JuxToolchain.resolveJuxcLsp`) rather
> than `PATH` alone; LSP4IJ is
> wired as an optional dependency with its own server factory rather than
> left to manual user setup; and the descriptor turns several LSP features
> off because the plugin's PSI serves them (see "Current State").

### Ultimate (native LSP API)

The plugin's `JuxLspServerSupportProvider` extends `com.intellij.platform.lsp.api.LspServerSupportProvider`:

```kotlin
class JuxLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(project: Project, file: VirtualFile, serverStarter: LspServerStarter) {
        if (file.fileType == JuxFileType) {
            serverStarter.ensureServerStarted(JuxLspDescriptor(project))
        }
    }
}

class JuxLspDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "Jux") {
    override fun isSupportedFile(file: VirtualFile): Boolean =
        file.fileType == JuxFileType

    override fun createCommandLine(): GeneralCommandLine =
        GeneralCommandLine("juxc-lsp")
}
```

The `juxc-lsp` binary is resolved from `$PATH`. A future enhancement (phase 4) bundles a platform-specific server binary inside the plugin distribution to remove the `$PATH` dependency.

### Community / non-Ultimate IDEs

`com.intellij.modules.lsp` is unavailable in Community editions. The `lsp.xml` config file referenced in `plugin.xml` is loaded only when that module is present, so the LSP-bound code is dead-weight on Community installs.

For Community users, the plugin documentation directs to **LSP4IJ** (JetBrains Marketplace, MIT-licensed): the plugin coexists with LSP4IJ, the user adds `juxc-lsp` as a server, and LSP4IJ routes the protocol traffic.

A more ambitious option is to bundle LSP4IJ's reusable runtime into this plugin so Community users get one-click LSP without a second install. This is **scope-deferred** to phase 4 pending licensing review.

---

## §I.7 — Refactoring Strategy

> **History.** The tiers below were the plan. In practice the plugin went
> straight to an expression-level PSI written by hand in Kotlin, not
> Grammar-Kit and JFlex, with tokens generated from `juxc-lex`. Rename, Find
> Usages, Safe Delete, the structure view, Introduce Variable, Introduce
> Constant and Inline Variable are PSI-backed today. Move Class, Extract
> Method, Change Signature, Inline Method and Structural Search and Replace
> are not built.

"Intelligent refactoring" in the IntelliJ sense decomposes into three tiers:

### Tier 1 — LSP-driven (phase 2)

Capabilities available without PSI, via the language server:

- **Rename** (`textDocument/rename`) — surfaced through the standard Refactor → Rename action via the LSP integration layer. Cross-file rename works because the server already tracks the import graph.
- **Find Usages** (`textDocument/references`) — surfaced through Edit → Find → Find Usages.
- **Goto Declaration / Implementation** (`textDocument/definition`, `…/implementation`).

These do NOT require any PSI. They are good enough for 80% of refactoring needs in a class-based language.

### Tier 2 — Declaration-level PSI (phase 3)

A shallow PSI tree covering only:

- `package` declarations
- `import` declarations
- Top-level `class` / `interface` / `enum` / `record` / `annotation` declarations
- Method signatures and field declarations inside the above
- Identifier nodes for any name binding listed above

Method bodies and expressions are stored as opaque text nodes. This is sufficient to support:

- **Move Class** — physically move the `.jux` file, rewrite its `package` line, rewrite `import` statements project-wide.
- **Safe Delete** — refuse if Find Usages returns matches; otherwise delete file.
- **Native Rename** — use IntelliJ's standard rename infrastructure (renames in comments, strings if user opts in) instead of the LSP path. Falls back to LSP for body-internal renames.
- **File-Structure popup** (`Cmd/Ctrl+F12`) showing class/method outline.

### Tier 3 — Full PSI (phase 4–5, conditional)

Expression-level PSI for:

- Extract Method / Extract Variable
- Change Method Signature
- Inline Variable / Inline Method
- Structural Search and Replace

This tier requires implementing a Jux parser in IntelliJ-platform-compatible Kotlin (likely via Grammar-Kit + JFlex). It is a **multi-month effort** and is gated on demonstrated user demand. The grammar-source for Grammar-Kit MUST be auto-generated from a single canonical source shared with the Rust parser — or the two parsers will drift. Generation infrastructure is itself a non-trivial sub-project, which is why this tier is conditional.

---

## §I.8 — PSI: When and How Much

> **History.** This matrix assumed PSI would be added in stages. The plugin
> has a full PSI now, so the "Requires PSI scope" column no longer gates
> anything; what remains open is listed under "Current State".

The decision matrix for adding PSI to the plugin:

| Refactoring user is asking for       | Requires PSI scope                | Phase |
|--------------------------------------|-----------------------------------|-------|
| Rename a variable inside one file    | None (LSP rename suffices)        | 2     |
| Rename a class across files          | None (LSP rename via import graph) | 2     |
| Find all references to a symbol      | None (LSP references)             | 2     |
| Move a class to a different package  | Declaration-level                 | 3     |
| Safely delete an unused class        | Declaration-level                 | 3     |
| Structure View / outline popup       | Declaration-level                 | 3     |
| Extract a method from selected code  | Expression-level                  | 4     |
| Change method signature              | Expression-level                  | 4     |
| Inline a constant                    | Expression-level                  | 5     |
| Structural Search and Replace        | Expression-level                  | 5     |

Phases 1 and 2 ship with zero PSI. Phase 3 introduces the declaration-level PSI and unlocks roughly half of the common refactorings. Phase 4 is conditional.

### Why this trade-off

A full PSI implementation duplicates the Rust front end's grammar in BNF and its lexer in JFlex regular expressions. Without code generation (the canonical Rust parser emitting both Rust and Grammar-Kit BNF), this duplication WILL drift. The plugin's value-per-line-of-code drops sharply as PSI grows. Stopping at declaration-level keeps the cost bounded and the value high.

---

## §I.9 — Distribution

### Marketplace

The plugin ships through the **JetBrains Marketplace** under the publisher `XTREME SOFTWARE SOLUTIONS` (publisher account to be reserved). Marketplace verification requires a `pluginIcon.svg`, a description, and screenshots. `./gradlew publishPlugin` reads everything secret from the environment of whoever runs it, and nothing secret is in the repository: `PUBLISH_TOKEN` for the Marketplace, and `CERTIFICATE_CHAIN`, `PRIVATE_KEY` and `PRIVATE_KEY_PASSWORD` for `signPlugin`.

### Bundled language server

A build can carry a `juxc-lsp` binary: `./gradlew buildPlugin -PjuxBundleLsp=<path to juxc-lsp>` (or `JUX_BUNDLE_LSP=<path>`) copies it into the plugin's `bin/` directory. It is off by default. The toolchain lookup tries the bundled binary last, after the configured toolchain, `$JUX_HOME`, `PATH` and the usual install locations, so an installed toolchain, whose server matches its own compiler, always wins. The binary is native code, so a bundled build is a build for one platform (see §I.11).

### Versioning

Plugin version mirrors the `juxc` toolchain minor version. `juxc 0.4.0` → plugin `0.4.x`. Patch releases are reserved for plugin-only fixes that don't bump the toolchain.

### Pre-release channel

Pre-release builds (`0.4.0-rc.1`, `0.4.0-rc.2`, …) ship on the marketplace's `beta` channel; stable releases go to `default`. The build picks the channel from `pluginVersion`: a version with a `-` suffix publishes to `beta`, any other to `default`. Users who opt into beta in `Settings → Plugins → ⚙ → Manage Plugin Repositories` get pre-releases automatically.

---

## §I.10 — Implementation Phases

> **History.** The original phase plan, kept for context. The plugin did not
> follow this order (it skipped the TextMate phase and built the PSI early).
> For what is done and what is open, see "Current State".

| Phase | Deliverable                                                                                                                  |
|-------|------------------------------------------------------------------------------------------------------------------------------|
| 1     | File type + TextMate grammar + source-root machinery + package inference + `${PACKAGE}` resolution + the six `New → Jux <thing>` actions + "Mark Directory as → Jux Sources Root" actions |
| 2     | LSP integration on Ultimate; LSP4IJ documentation for Community; Rename + Find Usages; package-mismatch inspection with quick-fixes; Project View Package View mode; auto-import quick-fix |
| 3     | Declaration-level PSI; Move Class (rewrites `package` + project-wide imports); Safe Delete; Structure View                  |
| 4     | Bundled `juxc-lsp` binary (no `$PATH` dep); LSP4IJ runtime embedded for Community                                            |
| 5     | Conditional: expression-level PSI (Extract Method, Change Signature, SSR)                                                    |

Phase 1 is the minimum useful plugin and is shippable in days, not weeks — and crucially it already includes Java-style package inference, so `New → Jux Class` produces a file with the correct `package` line on day one. Phase 2 multiplies the value at modest cost (the LSP exists by then per `JUX-LSP-SERVER-ADDENDUM.md`) and closes the "Java parity" gap with the package-mismatch inspection and Package View. Phase 3 is the largest single jump in user-perceived intelligence. Phase 4 is packaging polish. Phase 5 is the long tail.

---

## §I.11 — Open Questions

- **Grammar-source single-sourcing.** If Phase 5 is ever taken, the Rust parser and the Grammar-Kit BNF must derive from a single source — a `.lalrpop`-style grammar that compiles to both, or a Rust-side meta-grammar that emits the BNF. The design of that source is unspecified.
- **Android Studio / Rider compatibility.** The plugin's `<depends>` list is platform-only, so it SHOULD work in every IntelliJ-platform IDE. In practice, Android Studio's version-lag and Rider's plugin sandboxing have historically caused breakage. CI should publish to all targets and surface failures early.
- **Embedded `juxc-lsp` binaries.** Bundling the LSP binary inside the plugin (phase 4) means a per-platform plugin distribution (Win/Mac-x64/Mac-arm64/Linux-x64/Linux-arm64). The marketplace supports platform-specific builds; the build pipeline does not yet.
- **K2 mode.** Once Kotlin's K2 compiler mode is the default for IntelliJ plugins (target: 2026), the plugin should be re-verified under K2. No code changes anticipated, but the compiler swap is non-trivial historically.
