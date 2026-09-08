# Editor reach — VSCode, Zed, and one generated grammar

**Status:** planned, not started. Deferred deliberately: language correctness
comes first, and the compiler is where the bugs are. This file exists so the
design is not re-derived when it does come up.

## The point

Jux is **ahead** on language intelligence and **behind** on reach.
`juxc-lsp` already answers completion, hover, go-to-definition and
diagnostics, and the IntelliJ plugin has ~400 tests over a real PSI parser.
There is no VSCode extension, no Zed extension, and no website.

Three Rux repositories were cloned to `reference/` (gitignored) as a model —
`Zed`, `VSCode`, `Web`. What they show, beyond packaging shape: every one of
them **hand-maintains its keyword list**, in three separate copies that drift
the moment the language gains a keyword.

## 1. Generate `jux.tmLanguage.json` from `jux-tokens.json`

The first task, and the one everything else depends on.

`crates/juxc-driver/src/grammar_export.rs` already derives the lexer's token
inventory from `juxc-lex` and writes
`ide/intellij-plugin/grammar/jux-tokens.json` — 58 keywords, 24 primitives,
53 builtins, 9 annotations, plus operators and punctuation. The IntelliJ
plugin's token types are generated from it.

Extend that exporter to also emit a TextMate grammar. One generated file,
three consumers:

- the VSCode extension's `contributes.grammars`
- a markdown code-fence injection (below)
- Shiki, if a website is ever built

This keeps the rule the compiler already follows for the Rust standard
library: **the token list is discovered, never written down twice.** A new
keyword lands in `juxc-lex` and every editor learns it on the next export.

Shape to match: `reference/VSCode/syntaxes/rux.tmLanguage.json` (553 lines) —
but generated, and with Jux's own scopes. Note the pieces `jux-tokens.json`
does NOT carry and which the generator must add by hand: string interpolation
(`$"…${expr}…"` needs a nested pattern so the `${}` body highlights as code),
raw strings (`$"""…"""`), doc comments, and the annotation `@Name` form.

**Verification:** a test that every keyword in `jux-tokens.json` appears in
the generated grammar, so the two cannot drift — the same shape as the
example-coverage gate.

## 2. The VSCode extension

NOT a port of the reference. That one is syntax-only; Jux's would be:

- the generated TextMate grammar, for instant highlighting with no server
- `language-configuration.json` — brackets, comments, auto-closing pairs,
  and the `$"` auto-close the IntelliJ plugin already implements
- **an LSP client pointed at `juxc-lsp`**, which is the whole difference.
  Completion, diagnostics, go-to-definition and hover come free, and are
  already tested through the plugin's LSP path.

That makes it a better extension than the reference on day one, for a small
fraction of the work the IntelliJ plugin took.

## 3. Markdown code-fence injection

Thirty lines
(`reference/VSCode/syntaxes/rux.markdown.tmLanguage.json` is the model):
a grammar with `injectTo: ["text.html.markdown"]` that highlights ` ```jux `
blocks in any markdown file. Improves every doc in `Architecture/` — 24k
lines of specification, currently rendered as plain text — and every README.

## 4. Zed

After the generator exists. Two routes:

- **LSP-only extension.** Zed supports an extension that contributes just a
  language server, no grammar. Cheapest, and it suits Jux since `juxc-lsp` is
  the strong asset. Highlighting is then whatever Zed can do without a
  tree-sitter grammar, which is little — so this is the "reach now, polish
  later" option.
- **tree-sitter grammar** (`reference/Zed/grammar.js`, 224 lines, plus
  `highlights.scm` / `outline.scm` / `overrides.scm`). Full fidelity,
  materially more work, and a SECOND grammar to keep in step with the lexer —
  which is exactly the drift the generator exists to prevent. If it happens,
  generate it from `jux-tokens.json` too.

## 5. Website

Largest, last. `reference/Web` is Nuxt 4 + `@nuxt/content` + Shiki with 620
content files. The part worth copying is not the design: it is the
`verify:routes` / `verify:links` / `verify:meta` / `verify:render` scripts
and the Lighthouse configs. Docs get a CI gate the same way code does. If a
Jux site is built, it should be born with those.
