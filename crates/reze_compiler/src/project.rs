use std::collections::{BTreeMap, HashMap, VecDeque};

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use oxc_parser::Parser;
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::{SourceType, Span};

use crate::diagnostic::{self, Code, Report};
use crate::fs::{self, FileSystem};
use crate::routes;
use crate::{ModuleOutput, Options, ProjectOutput, analyze, client, emit, rust};

pub struct Module<'a> {
    pub path: String,
    pub source: &'a str,
    pub program: &'a Program<'a>,
    pub semantic: Semantic<'a>,
    /// Relative specifiers, and package specifiers with a `reze` export, as written, to the index of
    /// the module they resolve to.
    pub resolved: HashMap<String, usize>,
}

const EXTENSIONS: [&str; 4] = ["tsx", "ts", "jsx", "js"];

fn source_type(path: &str) -> SourceType {
    SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx())
}

/// Specifiers of every static import and re-export of `program`, with their spans.
fn specifiers<'a>(program: &Program<'a>) -> Vec<(&'a str, Span)> {
    let mut out = Vec::new();
    for statement in &program.body {
        let source = match statement {
            Statement::ImportDeclaration(decl) => &decl.source,
            Statement::ExportFromDeclaration(decl) => &decl.source,
            Statement::ExportAllDeclaration(decl) => &decl.source,
            _ => continue,
        };
        out.push((source.value.as_str(), source.span));
    }
    out
}

fn is_relative(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

fn resolve(fs: &dyn FileSystem, from: &str, specifier: &str) -> Option<(String, String)> {
    let joined = fs::join(from, specifier);
    let mut candidates = Vec::new();
    if let Some(stem) = [".js", ".jsx", ".mjs"].iter().find_map(|ext| joined.strip_suffix(ext)) {
        candidates.push(format!("{stem}.ts"));
        candidates.push(format!("{stem}.tsx"));
    }
    candidates.push(joined.clone());
    candidates.extend(EXTENSIONS.iter().map(|ext| format!("{joined}.{ext}")));
    candidates.extend(EXTENSIONS.iter().map(|ext| format!("{joined}/index.{ext}")));
    candidates.into_iter().find_map(|path| fs.read(&path).map(|source| (path, source)))
}

/// The `reze` export of a package specifier: `node_modules/<package>/<path>` and its source.
fn resolve_package(fs: &dyn FileSystem, specifier: &str) -> Option<(String, String)> {
    let mut parts = specifier.splitn(if specifier.starts_with('@') { 3 } else { 2 }, '/');
    let name = match (specifier.starts_with('@'), parts.next(), parts.next()) {
        (true, Some(scope), Some(name)) => format!("{scope}/{name}"),
        (false, Some(name), _) => name.to_string(),
        _ => return None,
    };
    let subpath = match specifier[name.len()..].strip_prefix('/') {
        Some(rest) => format!("./{rest}"),
        None => ".".to_string(),
    };
    let dir = format!("node_modules/{name}");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs.read(&format!("{dir}/package.json"))?).ok()?;
    let exports = manifest.get("exports")?;
    let conditions = match exports.get(&subpath) {
        Some(entry) => entry,
        None if subpath == "." && exports.get("reze").is_some() => exports,
        None => return None,
    };
    let target = conditions.get("reze")?.as_str()?;
    let path = fs::join(&format!("{dir}/package.json"), target);
    fs.read(&path).map(|source| (path, source))
}

pub fn compile(fs: &dyn FileSystem, options: &Options) -> ProjectOutput {
    let routes_dir = options.routes_dir.trim_end_matches('/');
    let parent = routes_dir.rsplit_once('/').map_or("", |(parent, _)| parent);
    let files = fs.list(parent);
    let (table, route_errors) = routes::scan(routes_dir, &files);

    let mut paths: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut reports: BTreeMap<usize, Vec<Report>> = BTreeMap::new();
    let mut queue = VecDeque::new();
    for node in &table.nodes {
        if let Some(source) = fs.read(&node.file) {
            index.insert(node.file.clone(), paths.len());
            queue.push_back(paths.len());
            paths.push(node.file.clone());
            sources.push(source);
        }
    }
    for error in route_errors {
        let Some(&module) = index.get(&error.file) else { continue };
        reports
            .entry(module)
            .or_default()
            .push(Report::new(Code::RouteInvalid, Span::empty(0)).arg("detail", error.message));
    }

    let mut edges: Vec<HashMap<String, usize>> = Vec::new();
    let mut parse_failed = false;
    let mut scratch = Allocator::default();
    while let Some(module) = queue.pop_front() {
        let source = std::mem::take(&mut sources[module]);
        let parsed = Parser::new(&scratch, &source, source_type(&paths[module])).parse();
        if let Some(error) = parsed.diagnostics.first() {
            parse_failed = true;
            let span = error.labels.first().map_or(Span::empty(0), |label| {
                Span::new(label.offset(), label.offset() + label.len())
            });
            reports
                .entry(module)
                .or_default()
                .push(Report::new(Code::ParseError, span).arg("detail", error.message.to_string()));
        }
        let mut resolved = HashMap::new();
        for (specifier, span) in specifiers(&parsed.program) {
            let found = if is_relative(specifier) {
                resolve(fs, &paths[module], specifier)
            } else {
                resolve_package(fs, specifier)
            };
            match found {
                Some((path, source)) => {
                    let target = *index.entry(path.clone()).or_insert_with(|| {
                        queue.push_back(paths.len());
                        paths.push(path);
                        sources.push(source);
                        paths.len() - 1
                    });
                    resolved.insert(specifier.to_string(), target);
                }
                None if is_relative(specifier) => reports
                    .entry(module)
                    .or_default()
                    .push(Report::new(Code::ImportUnresolved, span).arg("specifier", specifier)),
                None => {}
            }
        }
        if edges.len() <= module {
            edges.resize_with(module + 1, HashMap::new);
        }
        edges[module] = resolved;
        drop(parsed);
        sources[module] = source;
        scratch.reset();
    }
    edges.resize_with(paths.len(), HashMap::new);

    let types = emit::types(&table, parent);
    let mut output = ProjectOutput { routes: table, types, ..ProjectOutput::default() };
    if parse_failed || reports.values().flatten().any(|r| r.code.severity().is_error()) {
        output.diagnostics = resolve_reports(reports, &paths, &sources);
        return output;
    }

    let allocators: Vec<Allocator> = paths.iter().map(|_| Allocator::default()).collect();
    let modules: Vec<Module<'_>> = paths
        .iter()
        .zip(&sources)
        .zip(&allocators)
        .zip(edges)
        .map(|(((path, source), allocator), resolved)| {
            let parsed = Parser::new(allocator, source, source_type(path)).parse();
            let program: &Program<'_> = allocator.alloc(parsed.program);
            let semantic = SemanticBuilder::new().with_build_nodes(true).build(program).semantic;
            Module { path: path.clone(), source, program, semantic, resolved }
        })
        .collect();

    let mut analysis = analyze::run(&modules, &output.routes, options);
    for (module, mut module_reports) in
        std::mem::take(&mut analysis.reports).into_iter().enumerate()
    {
        reports.entry(module).or_default().append(&mut module_reports);
    }
    let analysis_failed = reports.values().flatten().any(|r| r.code.severity().is_error());
    let mut plan = None;
    if !analysis_failed {
        let mut client = client::plan(&modules, &analysis, &output.routes);
        for (module, mut module_reports) in
            std::mem::take(&mut client.reports).into_iter().enumerate()
        {
            reports.entry(module).or_default().append(&mut module_reports);
        }
        plan = Some(client);
    }
    let failed = reports.values().flatten().any(|r| r.code.severity().is_error());
    if let (Some(plan), Some(crates), false) = (&plan, &options.rust, failed) {
        let (code, rust_reports) =
            rust::emit(&modules, &analysis, &output.routes, plan, crates, routes_dir);
        for (module, mut module_reports) in rust_reports.into_iter().enumerate() {
            if !module_reports.is_empty() {
                reports.entry(module).or_default().append(&mut module_reports);
            }
        }
        output.rust = Some(code);
    }
    output.diagnostics = resolve_reports(reports, &paths, &sources);
    let Some(mut plan) = plan.filter(|_| !failed) else {
        return output;
    };
    for (i, module) in modules.iter().enumerate() {
        let server = emit::server(&modules, &analysis, i, &plan.modules[i]);
        let client = plan.modules[i].client.take();
        output.modules.insert(module.path.clone(), ModuleOutput { server: Some(server), client });
    }
    output.app = emit::app(&modules, &analysis, &output.routes, &plan.chunks);
    output.navigation = emit::navigation(&modules, &analysis, &output.routes);
    output.chunks = std::mem::take(&mut plan.chunks);
    output
}

fn resolve_reports(
    reports: BTreeMap<usize, Vec<Report>>,
    paths: &[String],
    sources: &[String],
) -> Vec<diagnostic::Diagnostic> {
    reports
        .into_iter()
        .flat_map(|(module, reports)| {
            diagnostic::resolve(reports, &sources[module], &paths[module])
        })
        .collect()
}
