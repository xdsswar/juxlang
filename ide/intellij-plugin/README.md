# Jux — IntelliJ Platform Plugin

Editor support for **Jux** in IntelliJ IDEA (and every other IntelliJ-platform
IDE: CLion, GoLand, PyCharm, RustRover, …). Implements Phase 1–2 of
`Architecture/JUX-INTELLIJ-PLUGIN-ADDENDUM.md` (§I).

## What it does

- **`.jux` file type** with its own icon (project view + editor tabs).
- **Java-like syntax coloring** — keywords, types, strings, numbers,
  annotations — via the bundled TextMate grammar (the canonical
  `editors/jux.tmLanguage.json`, shared with every other editor).
- **New → Jux File** with kinds, each with its own icon:
  *File, Class, Interface, Enum, Struct, Record, Annotation*.
- **Automatic `package`** inferred from the file's path relative to its
  source/content root (Java-style). Files outside any root omit the line.
- **Editable file header (copyright)** — every new file pulls in the
  `Jux File Header` include via Velocity `#parse`. Edit it once in
  **Settings → Editor → File and Code Templates → Includes → "Jux File Header"**
  and it applies to all new Jux files. Default uses `${YEAR}` and `${USER}`.
- **Run a Jux file** — a "Jux" run configuration runs `juxc <file> --run`. A
  context producer **autodetects `main`** (text scan for `void`/`int main(...)`)
  so files with an entry point get Run from the right-click menu, `Ctrl+Shift+F10`,
  and the toolbar Run button. `juxc` is resolved via **`$JUX_HOME`** → `PATH`
  (or an explicit path in the run config).
- **Semantic features via `juxc-lsp`** — diagnostics, hover types, completion,
  auto-import, served by the **native LSP client** (wired on Ultimate; gated so
  it's inert on Community-only IDEs). The plugin ships no language logic of its
  own; the Rust `juxc-lsp` server is the single source of truth.

## Toolchain

- **Gradle 9.1** (wrapper pinned) — run `./gradlew` / `gradlew.bat`.
- Target IDE: **IntelliJ IDEA 2026.1.3** (`intellijIdea("2026.1.3")`).
- **Build toolchain: JDK 21**, auto-provisioned by the foojay resolver in
  `settings.gradle.kts` (no manual JDK install needed).

> **Why JDK 21 and not 25?** IntelliJ's JBR is 25, so the IDE can load the
> result fine — but the Kotlin compiler maxes at JVM target 24 and the IntelliJ
> Platform Gradle Plugin rejects a JDK 25 toolchain. JDK 21 builds cleanly and
> the bytecode loads in the 2026.1.3 JBR with zero class-version risk.

Versions in `build.gradle.kts` (Kotlin `2.2.0`, platform plugin `2.16.0`) and
foojay `1.0.0` are the verified-working set for this Gradle 9 / 2026.1 line.

## Build & run

```bash
cd ide/intellij-plugin
./gradlew runIde        # launch a sandbox IDE with the plugin loaded
./gradlew buildPlugin   # produce build/distributions/<name>-<version>.zip
```

`runIde`/`buildPlugin` download the IntelliJ Platform on first run (network
required).

## Language server (`juxc-lsp`)

Semantic features need the `juxc-lsp` binary on `$PATH`. From the repo root:

```bash
cargo build --release -p juxc-lsp
# add target/release/ to PATH, or copy juxc-lsp(.exe) somewhere on PATH
```

- **IntelliJ Ultimate / paid IDEs** — the native LSP client
  (`JuxLspServerSupportProvider`) starts `juxc-lsp` automatically when a `.jux`
  file opens. It resolves the binary via `$JUX_HOME` → `PATH`. Loaded only when
  `com.intellij.modules.ultimate` is present (so it's inert elsewhere). This is
  wired and shipped in the plugin.
- **Community-only IDEs** — the native client is inert; install **LSP4IJ**
  (JetBrains Marketplace) and register `juxc-lsp` for the Jux file type. Same
  server, same features. (Not bundled yet.)

## Layout

```
ide/intellij-plugin/
├── build.gradle.kts            # IntelliJ Platform Gradle Plugin 2.x
├── gradle.properties           # version + platform target
├── gradle/ gradlew gradlew.bat # Gradle 9.1 wrapper
└── src/main/
    ├── kotlin/dev/jux/intellij/
    │   ├── JuxLanguage.kt   JuxFileType.kt   JuxIcons.kt   JuxPackageResolver.kt
    │   ├── actions/NewJuxFileAction.kt        # New → Jux File (+ kinds)
    │   ├── textmate/JuxBundleProvider.kt      # registers the TextMate grammar
    │   └── lsp/JuxLspServerSupportProvider.kt # native LSP (Ultimate)
    └── resources/
        ├── META-INF/plugin.xml + lsp.xml
        ├── icons/                              # 16px + @2x kind icons
        ├── fileTemplates/internal/             # New-file templates
        ├── fileTemplates/includes/             # "Jux File Header" (copyright)
        └── textmate/jux.tmbundle/              # bundled grammar
```
