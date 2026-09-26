use std::path::PathBuf;

use reze_compiler::{CATALOG, Code, Diagnostic, Example, Options, compile, render_skill};

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    match compile(source, "example.tsx", &Options::default()) {
        Ok(Some(out)) => out.diagnostics,
        Ok(None) => Vec::new(),
        Err(errors) => errors,
    }
}

fn skill_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/compiler/skills/reze-compiler-diagnostics/SKILL.md")
}

fn apply(source: &str, diagnostic: &Diagnostic) -> String {
    let mut edits: Vec<_> = diagnostic.fixes[0].edits.iter().collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut fixed = source.to_string();
    for edit in edits {
        fixed.replace_range(edit.start as usize..edit.end as usize, &edit.text);
    }
    fixed
}

fn has(diagnostics: &[Diagnostic], code: Code) -> bool {
    diagnostics.iter().any(|d| d.code == code)
}

#[test]
fn skill_is_current() {
    let path = skill_path();
    let skill = render_skill();
    if std::env::var_os("REZE_UPDATE_SKILL").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &skill).unwrap();
    }
    let written = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        written == skill,
        "{} is stale: run REZE_UPDATE_SKILL=1 cargo test -p reze_compiler --test catalog",
        path.display()
    );
}

#[test]
fn examples_are_true() {
    for entry in CATALOG {
        match entry.example {
            Example::Pair { bad, good } => {
                let found = diagnostics(bad);
                let diagnostic = found.iter().find(|d| d.code == entry.code);
                let diagnostic = diagnostic
                    .unwrap_or_else(|| panic!("{}: bad reports it: {found:?}", entry.name));
                assert_eq!(diagnostic.severity, entry.severity, "{}", entry.name);
                assert!(
                    !has(&diagnostics(good), entry.code),
                    "{}: good does not report it",
                    entry.name
                );
            }
            Example::Shows(module) => {
                assert!(
                    has(&diagnostics(module), entry.code),
                    "{}: the example reports it",
                    entry.name
                );
            }
        }
    }
}

#[test]
fn fixes_repair() {
    for entry in CATALOG {
        let Example::Pair { bad, .. } = entry.example else { continue };
        for diagnostic in
            diagnostics(bad).iter().filter(|d| d.code == entry.code && !d.fixes.is_empty())
        {
            let fixed = apply(bad, diagnostic);
            assert!(
                !has(&diagnostics(&fixed), entry.code),
                "{}: fixed source still reports it:\n{fixed}",
                entry.name
            );
        }
    }
}

/// Every `{key}` placeholder is filled; braces the template shows as text, like `{…}`, stay.
#[test]
fn messages_have_no_placeholders() {
    for entry in CATALOG {
        let (Example::Pair { bad: module, .. } | Example::Shows(module)) = entry.example;
        for diagnostic in diagnostics(module) {
            for text in std::iter::once(&diagnostic.message)
                .chain(diagnostic.fixes.iter().map(|fix| &fix.title))
            {
                let unfilled = text.split('{').skip(1).any(|rest| {
                    rest.split_once('}').is_some_and(|(key, _)| {
                        !key.is_empty() && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    })
                });
                assert!(!unfilled, "{}: {text}", entry.name);
            }
        }
    }
}

#[test]
fn every_code_has_one_section() {
    let skill = render_skill();
    let sections = skill.lines().filter(|line| {
        line.strip_prefix("## ")
            .is_some_and(|name| name.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
    });
    assert_eq!(sections.count(), CATALOG.len());
    assert!(skill.starts_with("---\nname: reze-compiler-diagnostics\n"));
}
