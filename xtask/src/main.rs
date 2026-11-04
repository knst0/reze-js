//! Repository tasks, run as `cargo xtask <task>`:
//!
//! - `skill [--check]`: writes the diagnostics skill from the compiler's catalog, or with
//!   `--check` fails when the written one is stale.

use std::path::Path;
use std::process::ExitCode;

const SKILL: &str = "packages/compiler/skills/reze-compiler-diagnostics/SKILL.md";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["skill"] => {
            std::fs::write(root.join(SKILL), reze_compiler::render_skill()).unwrap();
            ExitCode::SUCCESS
        }
        ["skill", "--check"] => {
            if std::fs::read_to_string(root.join(SKILL)).ok() == Some(reze_compiler::render_skill())
            {
                ExitCode::SUCCESS
            } else {
                eprintln!("{SKILL} is stale: run `cargo xtask skill`");
                ExitCode::FAILURE
            }
        }
        _ => {
            eprintln!("usage: cargo xtask skill [--check]");
            ExitCode::FAILURE
        }
    }
}
