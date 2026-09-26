// The Jux extension: `*.jux` is the `jux` language, coloured by the shared
// TextMate grammar, and every other answer (diagnostics, completion, hover,
// go-to, references, rename, symbols, inlay hints, semantic tokens) comes from
// `juxc-lsp`, the same server the IntelliJ plugin launches. The server takes
// no arguments and speaks LSP over stdio (JUX-LSP-SERVER-ADDENDUM §L.3), so
// this file only has to find it and start it.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  context.subscriptions.push(
    vscode.commands.registerCommand("jux.restartServer", async () => {
      await stopServer();
      await startServer(context);
    }),
  );
  await startServer(context);
}

export async function deactivate(): Promise<void> {
  await stopServer();
}

/** The configured server command: `jux.server.path`, `juxc-lsp` on PATH by default. */
function serverCommand(): string {
  const configured = vscode.workspace.getConfiguration("jux").get<string>("server.path", "juxc-lsp");
  return configured.trim() || "juxc-lsp";
}

async function startServer(context: vscode.ExtensionContext): Promise<void> {
  const command = serverCommand();
  const run = { command, transport: TransportKind.stdio };
  const serverOptions: ServerOptions = { run, debug: run };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "jux" },
      { scheme: "untitled", language: "jux" },
    ],
    synchronize: {
      // A manifest edit changes the package roots the server resolves against.
      fileEvents: vscode.workspace.createFileSystemWatcher("**/jux.toml"),
    },
  };

  const started = new LanguageClient("jux", "Jux", serverOptions, clientOptions);
  try {
    await started.start();
    client = started;
    context.subscriptions.push(started);
  } catch (err) {
    client = undefined;
    const message = err instanceof Error ? err.message : String(err);
    const choice = await vscode.window.showErrorMessage(
      `Jux: could not start the language server \`${command}\` (${message}). ` +
        "Highlighting still works; set `jux.server.path` to the juxc-lsp binary for the rest.",
      "Open Settings",
    );
    if (choice === "Open Settings") {
      await vscode.commands.executeCommand("workbench.action.openSettings", "jux.server.path");
    }
  }
}

async function stopServer(): Promise<void> {
  const running = client;
  client = undefined;
  if (running) {
    await running.stop();
  }
}
