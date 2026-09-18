//! The capability set the server advertises at `initialize` (§L.5).
//!
//! Extracted so it can be ASSERTED. A provider dropped in a refactor would
//! otherwise compile, ship, and simply make the feature stop existing in every
//! editor at once, with no test and no error: a client never asks for what the
//! server did not advertise.

use tower_lsp::lsp_types::*;

use crate::position::{encoding_kind, PositionEncoding};

/// The capabilities, with the position encoding negotiated for this client.
pub(crate) fn server_capabilities(enc: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        // §L.8: the column unit this session uses, echoed back to the client.
        position_encoding: Some(encoding_kind(enc)),
        // Incremental sync (§L.5): each change ships a range and its text,
        // applied to the document's rope.
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::INCREMENTAL),
            save: Some(TextDocumentSyncSaveOptions::Supported(true)),
            ..Default::default()
        })),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            // `.` member access, `:` path, `@` annotation (§L.5).
            trigger_characters: Some(vec![".".into(), ":".into(), "@".into()]),
            // Doc comments attach lazily via `completionItem/resolve`: only the
            // highlighted item pays the declaring-file read.
            resolve_provider: Some(true),
            ..Default::default()
        }),
        // Parameter info inside a call's `( … )`, re-triggered on each `,`.
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![",".into()]),
            work_done_progress_options: Default::default(),
        }),
        // Auto-import quick fixes for unresolved-but-known types.
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        // Goto-definition, into generated `rust.std` / crate stubs too.
        definition_provider: Some(OneOf::Left(true)),
        // An outline of the open file's declarations.
        document_symbol_provider: Some(OneOf::Left(true)),
        // Every use of a local, member or declaration across the project.
        references_provider: Some(OneOf::Left(true)),
        // Rename with a prepare step, so the editor learns up front whether
        // the name can be renamed and what text to offer.
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        // What each identifier is, for coloring (§L.5 precedence).
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: crate::semtok::legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: Some(true),
                work_done_progress_options: Default::default(),
            },
        )),
        // Inferred `var` types and parameter names at arguments.
        inlay_hint_provider: Some(OneOf::Left(true)),
        // The project's declarations by name.
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}
