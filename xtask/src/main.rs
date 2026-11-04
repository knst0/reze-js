use std::path::Path;
use std::process::{Command, ExitCode};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const SKILL: &str = "packages/compiler/skills/reze-compiler-diagnostics/SKILL.md";
const BLOG: &str = "tests/apps/blog/Cargo.toml";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["ci"] => ci(),
        ["skill"] => write_skill(),
        ["skill", "--check"] => check_skill(),
        _ => Err(String::from("usage: cargo xtask ci | skill [--check]")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn ci() -> Result<(), String> {
    run("pnpm", &["install", "--frozen-lockfile"])?;
    run("pnpm", &["build"])?;
    run("pnpm", &["fmt", "--check"])?;
    run("cargo", &["fmt", "--all", "--check"])?;
    run("cargo", &["fmt", "--all", "--check", "--manifest-path", BLOG])?;
    run("pnpm", &["lint"])?;
    run(
        "cargo",
        &["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"],
    )?;
    run("cargo", &["clippy", "--manifest-path", BLOG, "--", "-D", "warnings"])?;
    run("cargo", &["test", "--workspace"])?;
    check_skill()?;
    run("pnpm", &["--filter", "./examples/*", "build"])?;
    for example in examples()? {
        run("pnpm", &["exec", "tsc", "-p", &example])?;
    }
    run("pnpm", &["exec", "vp", "test", "run"])?;
    run("pnpm", &["exec", "playwright", "test"])?;
    run("node", &["scripts/bench/size.mts", "--check"])
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let command = format!("{program} {}", args.join(" "));
    eprintln!("\n> {command}");
    let status = Command::new(program)
        .args(args)
        .current_dir(ROOT)
        .status()
        .map_err(|error| format!("{command}: {error}"))?;
    if status.success() { Ok(()) } else { Err(format!("failed: {command}")) }
}

fn examples() -> Result<Vec<String>, String> {
    let entries =
        std::fs::read_dir(Path::new(ROOT).join("examples")).map_err(|error| error.to_string())?;
    let mut examples = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.path().is_dir() {
            examples.push(format!("examples/{}", entry.file_name().to_string_lossy()));
        }
    }
    examples.sort();
    Ok(examples)
}

fn write_skill() -> Result<(), String> {
    std::fs::write(Path::new(ROOT).join(SKILL), reze_compiler::render_skill())
        .map_err(|error| format!("{SKILL}: {error}"))
}

fn check_skill() -> Result<(), String> {
    match std::fs::read_to_string(Path::new(ROOT).join(SKILL)) {
        Ok(skill) if skill == reze_compiler::render_skill() => Ok(()),
        _ => Err(format!("{SKILL} is stale: run `cargo xtask skill`")),
    }
}
