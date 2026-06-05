//! Tiny driver: read a rustdoc-JSON file and print the generated `.jux.d`.
//!
//! Usage: `cargo run -p juxc-bindgen --example stub -- <rustdoc.json> [package]`
//! This is a scaffold for the eventual `juxc bindgen` subcommand (§G.6.2).

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: stub <rustdoc.json> [package]");
        return ExitCode::from(2);
    };
    let package = env::args().nth(2).unwrap_or_else(|| "rust.demo".to_string());

    let json = match fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("error reading {path}: {e}");
            return ExitCode::from(1);
        }
    };

    match juxc_bindgen::ingest::generate_from_json(&json, &package) {
        Ok(stub) => {
            print!("{}", juxc_bindgen::render_stub(&stub));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error parsing rustdoc JSON: {e}");
            ExitCode::from(1)
        }
    }
}
