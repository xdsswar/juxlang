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
mod intel;
mod position;
mod scope;
mod server;
mod workspace;

use server::Backend;
use tower_lsp::{LspService, Server};

/// Name of the project-local `.jux.d` stub cache directory (JUX-BINDGEN §G.11.2).
/// Re-exported from `juxc-driver` so the workspace scan and the driver's stub
/// loader agree on the one directory name.
pub(crate) fn stubs_dirname() -> &'static str {
    juxc_driver::stubs::PROJECT_STUB_DIRNAME
}

fn main() {
    // The analysis path is the compiler front end, which recurses in step with
    // source nesting and has large frames. Tokio's worker threads default to a
    // 2 MB stack -- less than a main thread gets -- so a deeply nested file
    // would abort the whole language server while the user was typing in it.
    // Reserve the same headroom the batch compiler uses.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(juxc_driver::big_stack::STACK_SIZE)
        .enable_all()
        .build()
        .expect("building the tokio runtime");
    runtime.block_on(serve());
}

/// The server's async body: LSP over stdio, until the client disconnects.
async fn serve() {
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
