//! `juxc-lsp` — the Jux language server binary.
//!
//! Per `JUX-LSP-SERVER-ADDENDUM.md` §L, this server reuses the Jux front-end
//! crates (`juxc-driver` → lex/parse/resolve/tycheck) behind a thin LSP shim.
//! It contains **no** parser or type checker of its own: every semantic answer
//! (diagnostics, hover, completion) comes from the same crates that drive the
//! batch compiler.
//!
//! Transport is LSP over **stdio** (§L.3). The server MUST NOT write to stdout
//! for anything but JSON-RPC frames — logging goes to stderr.

mod analysis;
mod diagnostics;
mod doc;
mod position;
mod server;
mod workspace;

use server::Backend;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    // stdio transport. `tokio::io::stdin/stdout` give us the async byte
    // streams tower-lsp frames JSON-RPC over.
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    // `Backend::new` is `Fn(Client) -> Backend`, exactly the shape
    // `LspService::new` wants.
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    // `serve` returns when the client disconnects (stdio pipe closed because
    // the IDE exited) or sends the LSP `exit` notification. Force-exit so the
    // process never lingers holding the binary open — a zombie server would
    // both waste resources and lock `juxc-lsp.exe` against rebuilds.
    std::process::exit(0);
}
