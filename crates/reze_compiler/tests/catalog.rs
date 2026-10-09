use std::path::PathBuf;

use reze_compiler::{
    CATALOG, Code, CompileTarget, Diagnostic, Example, Options, compile, render_skill,
};

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    match compile(source, "example.tsx", &Options::default()) {
        Ok(Some(out)) => out.diagnostics,
        Ok(None) => Vec::new(),
        Err(errors) => errors,
    }
}

/// Options that trigger `code` for its catalog example. Every code is source-gated under the
/// default options except `MISSING_MODULE_ID`, which is option-gated: its example only reports
/// with an `island`/`html` target and no `moduleId`.
fn example_options(code: Code) -> Options {
    match code {
        Code::MissingModuleId => Options { target: CompileTarget::Island, ..Options::default() },
        _ => Options::default(),
    }
}

fn diagnostics_for(source: &str, options: &Options) -> Vec<Diagnostic> {
    match compile(source, "example.tsx", options) {
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
        let options = example_options(entry.code);
        match entry.example {
            Example::Pair { bad, good } => {
                let found = diagnostics_for(bad, &options);
                let diagnostic = found.iter().find(|d| d.code == entry.code);
                let diagnostic = diagnostic
                    .unwrap_or_else(|| panic!("{}: bad reports it: {found:?}", entry.name));
                assert_eq!(diagnostic.severity, entry.severity, "{}", entry.name);
                assert!(
                    !has(&diagnostics_for(good, &options), entry.code),
                    "{}: good does not report it",
                    entry.name
                );
            }
            Example::Shows(module) => {
                assert!(
                    has(&diagnostics_for(module, &options), entry.code),
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
        let found: Vec<_> = diagnostics(bad)
            .into_iter()
            .filter(|d| d.code == entry.code && !d.fixes.is_empty())
            .collect();
        assert!(
            entry.fix.is_none() || !found.is_empty(),
            "{}: the bad example gets a fix",
            entry.name
        );
        for diagnostic in &found {
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
        for diagnostic in diagnostics_for(module, &example_options(entry.code)) {
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
