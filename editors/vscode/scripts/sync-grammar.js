// Copies the shared TextMate grammar (editors/jux.tmLanguage.json) into
// syntaxes/, where package.json's `contributes.grammars` points, and the
// repository's LICENSE next to package.json, where `vsce package` looks for it.
//
// Both files have ONE home in the repository, and `vsce package` cannot reach
// a file outside the extension's own folder. So the copies are made here, at
// compile time, and never committed (see .gitignore): the extension can never
// ship a stale grammar.
"use strict";

const fs = require("fs");
const path = require("path");

const here = path.resolve(__dirname, "..");
const copies = [
  [path.resolve(here, "..", "jux.tmLanguage.json"), path.resolve(here, "syntaxes", "jux.tmLanguage.json")],
  [path.resolve(here, "..", "..", "LICENSE"), path.resolve(here, "LICENSE")],
];

for (const [source, target] of copies) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(source, target);
  console.log(`${path.relative(here, source)} -> ${path.relative(here, target)}`);
}
