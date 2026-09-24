//! Multi-module fixtures (SPEC §15.13): every `tests/programs/<name>/` goes through
//! `summarize` → `link` → `compile` for the three targets; the snapshot holds every output, the
//! info diagnostics, and what `link` decided for the build.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, ImportDeclarationSpecifier, Statement};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{
    LinkOptions, ModuleInput, Options, SummaryOptions, Target, compile, link, summarize,
};

const TARGETS: &[(Target, &str)] =
    &[(Target::Client, "client"), (Target::Server, "server"), (Target::Hydrate, "hydrate")];

const EXTENSIONS: &[&str] = &["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx"];

/// `program.json`: entry files, and the link options.
struct Config {
    entries: Vec<String>,
    islands: bool,
    optimize: bool,
}

fn config(dir: &Path) -> Config {
    let text = std::fs::read_to_string(dir.join("program.json")).unwrap_or_else(|_| "{}".into());
    let json: serde_json::Value = serde_json::from_str(&text).expect("program.json");
    Config {
        entries: json["entries"]
            .as_array()
            .map(|a| a.iter().map(|e| format!("/{}", e.as_str().unwrap())).collect())
            .unwrap_or_default(),
        islands: json["islands"].as_bool().unwrap_or(false),
        optimize: json["optimize"].as_bool().unwrap_or(true),
    }
}

/// Sources by id: `/<path relative to the fixture>`.
fn sources(dir: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if path
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("ts" | "tsx" | "js" | "jsx")))
            {
                let id = format!(
                    "/{}",
                    path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")
                );
                out.insert(id, std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn resolve(from: &str, specifier: &str, ids: &BTreeMap<String, String>) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let mut parts: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let base = format!("/{}", parts.join("/"));
    EXTENSIONS.iter().map(|e| format!("{base}{e}")).find(|id| ids.contains_key(id))
}

fn assert_valid(code: &str, id: &str) {
    let source_type = SourceType::from_path(id).unwrap();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    assert!(parsed.diagnostics.is_empty(), "{id}: {:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{id}: {:?}\n{code}", semantic.diagnostics);
}

/// Named imports of each module (specifier, imported name) and the names it exports; `None` for
/// the exports of a module with `export *`.
fn module_names(code: &str, id: &str) -> (Vec<(String, String)>, Option<BTreeSet<String>>) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, SourceType::from_path(id).unwrap()).parse();
    let mut imports = Vec::new();
    let mut exports = Some(BTreeSet::new());
    macro_rules! export {
        ($name:expr) => {
            if let Some(exports) = &mut exports {
                exports.insert($name);
            }
        };
    }
    for statement in &parsed.program.body {
        match statement {
            Statement::ImportDeclaration(import) if !import.import_kind.is_type() => {
                for specifier in import.specifiers.iter().flatten() {
                    let name = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(s)
                            if !s.import_kind.is_type() =>
                        {
                            s.imported.name().to_string()
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => "default".into(),
                        _ => continue,
                    };
                    imports.push((import.source.value.to_string(), name));
                }
            }
            Statement::ExportDeclaration(declaration) => match &declaration.declaration {
                Declaration::VariableDeclaration(variables) => {
                    for declarator in &variables.declarations {
                        for id in declarator.id.get_binding_identifiers() {
                            export!(id.name.to_string());
                        }
                    }
                }
                Declaration::FunctionDeclaration(f) => {
                    f.id.iter().for_each(|id| export!(id.name.to_string()))
                }
                Declaration::ClassDeclaration(c) => {
                    c.id.iter().for_each(|id| export!(id.name.to_string()))
                }
                _ => {}
            },
            Statement::ExportNamedDeclaration(named) => {
                named.specifiers.iter().for_each(|s| export!(s.exported.name().to_string()));
            }
            Statement::ExportFromDeclaration(from) => {
                from.specifiers.iter().for_each(|s| export!(s.exported.name().to_string()));
            }
            Statement::ExportDefaultDeclaration(_) => export!("default".into()),
            Statement::ExportAllDeclaration(all) => match &all.exported {
                Some(name) => export!(name.name().to_string()),
                None => exports = None,
            },
            _ => {}
        }
    }
    (imports, exports)
}

/// Every named import of a program module names an export of that module's output for the same
/// target: no cross-module rewrite removes an export another module still imports.
fn assert_imports_resolve(outputs: &BTreeMap<String, String>, label: &str) {
    let names: BTreeMap<&str, _> =
        outputs.iter().map(|(id, code)| (id.as_str(), module_names(code, id))).collect();
    for (id, (imports, _)) in &names {
        for (specifier, name) in imports {
            let Some(target) = resolve(id, specifier, outputs) else { continue };
            if let (_, Some(exports)) = &names[target.as_str()] {
                assert!(
                    exports.contains(name),
                    "{id} [{label}] imports `{name}` from {target}, whose output does not export it"
                );
            }
        }
    }
}

fn link_program(dir: &Path) -> (Config, BTreeMap<String, String>, reze_compiler::Linked) {
    let config = config(dir);
    let ids = sources(dir);
    let modules: Vec<ModuleInput> = ids
        .iter()
        .map(|(id, source)| {
            let summary = summarize(source, id, &SummaryOptions::default())
                .unwrap_or_else(|e| panic!("{id}: {e:?}"));
            let json = serde_json::to_string(&summary).unwrap();
            let summary: reze_compiler::ModuleSummary =
                serde_json::from_str(&json).expect("summaries round-trip through JSON");
            let resolved = summary.specifiers.iter().map(|s| resolve(id, s, &ids)).collect();
            ModuleInput { id: id.clone(), summary, resolved, is_entry: config.entries.contains(id) }
        })
        .collect();
    let linked = link(
        &modules,
        &LinkOptions { optimize: config.optimize, islands: config.islands, root: "/".to_string() },
    );
    (config, ids, linked)
}

fn render(dir: &Path) -> String {
    let (config, ids, linked) = link_program(dir);
    let mut out = format!("closed: {:?}\nfeatures: {:?}\n", linked.closed, linked.features);
    let mut outputs: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
    for (id, source) in &ids {
        let facts = linked.facts.get(id).expect("facts for every module").clone();
        let json = serde_json::to_string(&facts).unwrap();
        let facts: reze_compiler::ModuleFacts =
            serde_json::from_str(&json).expect("facts round-trip through JSON");
        for (target, label) in TARGETS {
            let options = Options {
                source_map: false,
                optimize: config.optimize,
                target: *target,
                facts: Some(facts.clone()),
                ..Options::default()
            };
            out.push_str(&format!("\n// ==== {id} [{label}] ====\n"));
            match compile(source, id, &options) {
                Ok(None) => {
                    out.push_str("<unchanged>\n");
                    outputs.entry(label).or_default().insert(id.clone(), source.clone());
                }
                Ok(Some(output)) => {
                    assert_valid(&output.code, id);
                    out.push_str(&output.code);
                    outputs.entry(label).or_default().insert(id.clone(), output.code.clone());
                    if *target == Target::Client {
                        for diagnostic in &output.diagnostics {
                            out.push_str("\n// ");
                            out.push_str(diagnostic.severity.as_str());
                            out.push('\n');
                            out.push_str(&diagnostic.rendered);
                        }
                    }
                }
                Err(errors) => {
                    for error in errors {
                        out.push_str(&error.rendered);
                        out.push('\n');
                    }
                }
            }
        }
    }
    for (label, outputs) in &outputs {
        assert_imports_resolve(outputs, label);
    }
    out
}

#[test]
fn program_snapshots() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/programs");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        insta::assert_snapshot!(format!("program__{name}"), render(&dir));
    }
}

#[test]
fn an_island_needs_the_features_of_the_components_it_renders() {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/programs/island_child_features");
    let (_, _, linked) = link_program(&dir);
    let [island] = linked.facts["/Page.tsx"].islands.as_slice() else {
        panic!("Page renders one island: {:?}", linked.facts["/Page.tsx"].islands);
    };
    assert!(island.features.iter().any(|f| f == "Loading"), "{:?}", island.features);
}
