# Installing Jux

This guide covers building the Jux toolchain from source, putting it on your
system, and installing the IntelliJ plugin so you get syntax highlighting,
diagnostics, completion, and a Run button for `.jux` files.

> Repository: <https://github.com/xdsswar/juxlang>

---

## 1. What you'll install

| Component    | What it is                                              | Needed for                         |
|--------------|---------------------------------------------------------|------------------------------------|
| `juxc`       | The Jux compiler (file-level: compile / build / run)    | Building and running Jux programs  |
| `jux`        | The Jux project tool                                    | Project-level builds               |
| `juxc-lsp`   | The Jux language server                                 | IDE diagnostics / hover / completion |
| IntelliJ plugin | Editor integration for IntelliJ-platform IDEs        | Working in the IDE                 |

The IDE plugin is a thin client: all the smart features come from `juxc-lsp`.
Install the binaries first, then the plugin.

---

## 2. Prerequisites

- **Rust** (stable) with `cargo` — install via <https://rustup.rs>.
  The repo pins a stable toolchain (`rust-toolchain.toml`); `rustup` honours it
  automatically.
- **Git**, to clone the repo.
- **An IntelliJ-platform IDE 2024.1 or newer** (IntelliJ IDEA, CLion, GoLand,
  PyCharm, RustRover, …). The prebuilt plugin targets **2026.1.3**.
- *(Optional)* a **Rust nightly** toolchain — only needed if you generate Rust
  crate bindings yourself (`rustdoc` JSON, see §7). Not required for normal use.

```sh
rustc --version   # should print a stable toolchain
cargo --version
```

---

## 3. Build the toolchain

Clone and build the three binaries in release mode:

```sh
git clone https://github.com/xdsswar/juxlang
cd juxlang
cargo build --release -p juxc -p jux -p juxc-lsp
```

The binaries land in `target/release/`:

| Platform        | Files                                              |
|-----------------|----------------------------------------------------|
| Windows         | `juxc.exe`, `jux.exe`, `juxc-lsp.exe`              |
| macOS / Linux   | `juxc`, `jux`, `juxc-lsp`                          |

---

## 4. Install the binaries and set `JUX_HOME`

The tools (and the IntelliJ plugin) locate `juxc` / `juxc-lsp` in this order:

1. an explicit path you configure (in a run configuration), then
2. **`$JUX_HOME/bin/`** (or `$JUX_HOME/`), then
3. your `PATH`.

The recommended setup is to copy the binaries into a `JUX_HOME/bin` directory
and point the `JUX_HOME` environment variable at the root.

### Windows (PowerShell)

```powershell
# Choose an install root, e.g. C:\Tools\jux
$JuxHome = "C:\Tools\jux"
New-Item -ItemType Directory -Force -Path "$JuxHome\bin" | Out-Null
Copy-Item target\release\juxc.exe,target\release\jux.exe,target\release\juxc-lsp.exe "$JuxHome\bin"

# Persist JUX_HOME and add the bin dir to PATH (user scope).
setx JUX_HOME $JuxHome
setx PATH "$env:PATH;$JuxHome\bin"
```

Open a **new** terminal afterward so the variables take effect.
Restart IntelliJ too, so it picks up the new `JUX_HOME`.

### macOS / Linux (bash/zsh)

```sh
JUX_HOME="$HOME/.jux"
mkdir -p "$JUX_HOME/bin"
cp target/release/juxc target/release/jux target/release/juxc-lsp "$JUX_HOME/bin/"

# Add to your shell profile (~/.bashrc or ~/.zshrc):
echo 'export JUX_HOME="$HOME/.jux"' >> ~/.zshrc
echo 'export PATH="$JUX_HOME/bin:$PATH"' >> ~/.zshrc
```

Open a new shell (or `source ~/.zshrc`).

---

## 5. Verify the CLI

```sh
juxc --version
```

Create a `hello.jux`:

```jux
public void main() {
    // your first Jux program
}
```

Compile and run it (the `--run` flag builds and executes, forwarding output and
the exit code):

```sh
juxc hello.jux --run
```

> `juxc` lowers Jux to Rust and uses `cargo` under the hood, so the first run
> compiles a small Rust crate. Subsequent runs reuse the cached build.

---

## 6. Install the IntelliJ plugin

### Option A — use the prebuilt zip

If you have the built artifact at
`ide/intellij-plugin/build/distributions/jux-intellij-0.0.1.zip`, skip to
**Install in the IDE** below.

### Option B — build the plugin

```sh
cd ide/intellij-plugin
./gradlew buildPlugin        # Windows: .\gradlew.bat buildPlugin
```

The first build downloads the IntelliJ Platform and (via the foojay resolver) a
**JDK 21** toolchain — no manual JDK install needed. The result is
`build/distributions/jux-intellij-0.0.1.zip`.

> The plugin builds with **Gradle 9.1** and targets **JDK 21 bytecode**, which
> loads cleanly in IntelliJ's JBR. See `ide/intellij-plugin/README.md` for the
> toolchain rationale.

### Install in the IDE

1. **Settings/Preferences → Plugins → ⚙ (gear) → Install Plugin from Disk…**
2. Select `jux-intellij-0.0.1.zip`.
3. **Restart the IDE.**

---

## 7. Verify in the IDE

Open a `.jux` file (or create one with **New → Jux File**). You should see:

- **Syntax highlighting** — keywords, types, strings, annotations (Java-like).
- **New → Jux File** with kinds: File, Class, Interface, Enum, Struct, Record,
  Annotation — each with its own icon, and an auto-filled `package` line.
- **Editable copyright header** on new files — change it once in
  **Settings → Editor → File and Code Templates → Includes → "Jux File Header"**.
- **A Run option** for any file containing a `main` — right-click → Run,
  `Ctrl+Shift+F10`, or the toolbar Run button. It runs `juxc <file> --run`.
- **Language server** — open a `.jux` file and check
  **Settings → Languages & Frameworks → Language Servers** (or the *Language
  Servers* tool window) for `Jux` running. Diagnostics, hover types, and
  completion come from it.

> On **IntelliJ Community** the native LSP client is inert; install **LSP4IJ**
> from the Marketplace and register `juxc-lsp` for the Jux file type to get the
> same features. On **Ultimate / paid IDEs** it's automatic.

---

## 8. (Optional) Generate Rust crate bindings

`juxc-bindgen` turns a Rust crate's public API into Jux-syntax stubs (`.jux.d`)
so you can use Rust crates with Jux completion. This currently works from a
`rustdoc` JSON file (a `juxc bindgen` subcommand is planned). It needs a Rust
**nightly** toolchain to produce the JSON:

```sh
rustup toolchain install nightly

# Produce rustdoc JSON for a crate's source, then generate the stub:
rustup run nightly rustdoc --output-format json -Z unstable-options \
    --crate-name mycrate src/lib.rs --out-dir .
cargo run -p juxc-bindgen --example stub -- mycrate.json rust.mycrate
```

The printed `.jux.d` is a signature-only Jux view of the crate's API.

---

## 9. Troubleshooting

| Symptom | Fix |
|---------|-----|
| `juxc: command not found` | Open a new terminal after `setx`/profile edit; confirm `JUX_HOME/bin` is on `PATH`. |
| Run does nothing / "cannot run program" | `juxc` not found — set `JUX_HOME` (and restart the IDE so it inherits the variable). |
| Language Servers shows `juxc-lsp` *failed to start* | `juxc-lsp` not on `JUX_HOME/bin` or `PATH`; build it (`cargo build --release -p juxc-lsp`) and restart the IDE. |
| No smart features on **Community** IDE | Native LSP is Ultimate-only; install **LSP4IJ** and register `juxc-lsp`. |
| Plugin won't install | IDE must be 2024.1+; the prebuilt plugin targets 2026.1.3. |
| `juxc --run` fails to build the emitted crate | Ensure `cargo`/`rustc` are installed and on `PATH` — `juxc` shells out to them. |

---

*The plugin can't crash the IDE: a missing or failing `juxc-lsp` only shows as a
stopped server in the Language Servers tool window, and the editor keeps
working (highlighting, New actions, Run) regardless.*
