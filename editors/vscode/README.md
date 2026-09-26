# Jux for VS Code

Editor support for **Jux** in Visual Studio Code. Unpublished: build it from
this folder and install the `.vsix` locally.

## What it does

- Registers `*.jux` as the `jux` language, with comment toggling, bracket
  matching and auto-closing pairs (`language-configuration.json`).
- Colours it with the shared TextMate grammar, `editors/jux.tmLanguage.json`,
  the one the TextMate bundle and every other editor use. Highlighting needs
  no server.
- Launches **`juxc-lsp`** over stdio for everything else: diagnostics,
  completion, hover, go-to-definition, references, rename, document and
  workspace symbols, inlay hints and semantic tokens. The extension holds no
  language logic of its own; the server is the same one the IntelliJ plugin
  starts.

## Settings

| Setting | Default | Meaning |
|---|---|---|
| `jux.server.path` | `juxc-lsp` | The server binary. A bare name is looked up on `PATH`; otherwise give an absolute path, e.g. `C:\\jux\\bin\\juxc-lsp.exe`. |
| `jux.trace.server` | `off` | `messages` or `verbose` logs the JSON-RPC traffic to the **Jux** output channel. |

After changing `jux.server.path`, run **Jux: Restart Language Server** from
the command palette. If the server cannot be started, highlighting keeps
working and a notification offers to open the setting.

## Building

Needs Node.js 18 or newer. From this folder:

```sh
npm install
npm run compile          # copies the grammar into syntaxes/, then runs tsc
```

`npm run compile` copies `../jux.tmLanguage.json` into `syntaxes/` and the
repository's `LICENSE` next to `package.json`. Neither copy is committed
(see `.gitignore`), so edit the grammar only in `editors/jux.tmLanguage.json`.

To build `juxc-lsp` itself, from the repository root:

```sh
cargo build --release -p juxc-lsp   # target/release/juxc-lsp(.exe)
```

## Packaging and installing locally

```sh
npm run package          # runs vsce package; writes jux-0.1.0.vsix
code --install-extension jux-0.1.0.vsix
```

Or, in VS Code: **Extensions → ⋯ → Install from VSIX…** and pick the file.
`npm run package` runs `vscode:prepublish` first, so the `.vsix` always
carries the current grammar.

To try it without installing, open this folder in VS Code and press **F5**
(Run Extension), which starts an Extension Development Host with the
extension loaded; run `npm run compile` first.
