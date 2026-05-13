//! Build script for `juxc` — embeds Windows `VERSIONINFO` + icon into
//! the produced `juxc.exe` on Windows hosts. On non-Windows hosts it
//! is a no-op.
//!
//! The icon lives at the repo root (`<repo>/juxc.ico`); we reference
//! it via `../../juxc.ico` relative to this crate's manifest dir.
//! Version metadata comes from the workspace `[package]` — Cargo
//! passes the package version and description in as env vars before
//! the build script runs.

fn main() {
    // Surface the icon as a `rerun-if-changed` input so editing the
    // `.ico` file forces a rebuild (otherwise Cargo only watches the
    // build script + sources).
    println!("cargo:rerun-if-changed=../../juxc.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../../juxc.ico");
        // VERSIONINFO fields. winres autopopulates `FileVersion` /
        // `ProductVersion` from `CARGO_PKG_VERSION` and falls back
        // to the package's authors for `LegalCopyright`, but we
        // override the strings we care about explicitly so the
        // output is stable regardless of metadata edits.
        res.set("ProductName", "Juxc — Jux Compiler");
        res.set("FileDescription", "The Jux compiler (juxc)");
        res.set("CompanyName", "XTREME SOFTWARE SOLUTIONS");
        res.set(
            "LegalCopyright",
            "Copyright (c) XDSSWAR / XTREME SOFTWARE SOLUTIONS",
        );
        res.set("OriginalFilename", "juxc.exe");
        res.set("InternalName", "juxc");
        if let Err(e) = res.compile() {
            // Don't fail the build on a missing resource compiler —
            // it just means no icon/versioninfo on this host. Surface
            // the cause so the user can debug if they care.
            println!(
                "cargo:warning=winres: failed to embed resources for juxc: {e}"
            );
        }
    }
}
