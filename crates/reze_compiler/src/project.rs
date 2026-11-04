use std::collections::{BTreeMap, HashMap, VecDeque};

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Declaration, Program, Statement};
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
    let mut project = options.rust.then(|| {
        let names = rust::node_names(&table, routes_dir);
        let nodes: Vec<(usize, String, String)> = table
            .nodes
            .iter()
            .zip(names)
            .enumerate()
            .map(|(index, (node, name))| (index, node.file.clone(), name))
            .collect();
        let server =
            if parent.is_empty() { "server.rs".into() } else { format!("{parent}/server.rs") };
        rust::source::extract(fs, &nodes, &files, routes_dir, &server)
    });

    let mut paths: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut reports: BTreeMap<usize, Vec<Report>> = BTreeMap::new();
    let mut queue = VecDeque::new();
    let mut suffixes: HashMap<usize, (usize, bool, bool)> = HashMap::new();
    for node in &table.nodes {
        if let Some(mut source) = fs.read(&node.file) {
            if let Some(route) = project
                .as_ref()
                .and_then(|p| p.route_of_file(&rust::source::rust_sibling(&node.file)))
            {
                suffixes.insert(paths.len(), (source.len(), route.load.is_some(), route.has_guard));
                source.push_str(&rust::typescript::route_suffix(route));
            }
            index.insert(node.file.clone(), paths.len());
            queue.push_back(paths.len());
            paths.push(node.file.clone());
            sources.push(source);
        }
    }
    if let Some(project) = &project {
        for route in project.routes.values() {
            index.insert(route.file.clone(), paths.len());
            queue.push_back(paths.len());
            paths.push(route.file.clone());
            sources.push(rust::typescript::module(project, route));
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
        let suffix = suffixes.get(&module).map(|&(suffix, _, _)| suffix);
        if let Some(&(suffix, has_load, has_guard)) = suffixes.get(&module) {
            for (name, span) in exported_names(&parsed.program) {
                let is_taken = (name == "load" && has_load) || (name == "guard" && has_guard);
                if is_taken && (span.start as usize) < suffix {
                    reports.entry(module).or_default().push(
                        Report::new(Code::RustRouteInvalid, span).arg(
                            "reason",
                            format!("The `.rs` file beside this route module runs its `{name}`: remove this `{name}`"),
                        ),
                    );
                }
            }
        }
        let user_error = parsed.diagnostics.iter().find(|error| {
            let offset = error.labels.first().map_or(0, |label| label.offset());
            suffix.is_none_or(|suffix| (offset as usize) < suffix)
        });
        if let Some(error) = user_error.or(parsed.diagnostics.first()) {
            parse_failed = true;
            let span = error.labels.first().map_or(Span::empty(0), |label| {
                Span::new(label.offset(), label.offset() + label.len())
            });
            if suffix.is_none_or(|suffix| (span.start as usize) < suffix) {
                reports.entry(module).or_default().push(
                    Report::new(Code::ParseError, span).arg("detail", error.message.to_string()),
                );
            }
        }
        let mut resolved = HashMap::new();
        for (specifier, span) in specifiers(&parsed.program) {
            if is_relative(specifier) && specifier.ends_with(".rs") {
                match index.get(&fs::join(&paths[module], specifier)) {
                    Some(&target) if project.is_some() => {
                        resolved.insert(specifier.to_string(), target);
                    }
                    _ => reports.entry(module).or_default().push(
                        Report::new(Code::ImportUnresolved, span).arg("specifier", specifier),
                    ),
                }
                continue;
            }
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

    let rust_data: Vec<String> = match &project {
        Some(project) => table
            .nodes
            .iter()
            .filter(|node| {
                project
                    .route_of_file(&rust::source::rust_sibling(&node.file))
                    .is_some_and(|route| route.load.is_some())
            })
            .map(|node| node.file.clone())
            .collect(),
        None => Vec::new(),
    };
    let types = emit::types(&table, parent, &rust_data);
    let mut output = ProjectOutput { routes: table, types, ..ProjectOutput::default() };
    let rust_failed = project.as_ref().is_some_and(rust::source::Project::has_errors);
    let mut rust_diagnostics = Vec::new();
    if let Some(project) = &mut project {
        for file in &mut project.files {
            let reports = std::mem::take(&mut file.reports);
            rust_diagnostics.extend(diagnostic::resolve(reports, &file.source, &file.path));
        }
        if !rust_failed {
            let types = project
                .routes
                .values()
                .map(|route| {
                    let stem = route.file.strip_suffix(".rs").unwrap_or(&route.file);
                    (format!("{stem}.d.rs.ts"), rust::typescript::declarations(project, route))
                })
                .collect();
            output.rust = Some(crate::RustOutput { main: None, types });
        }
    }
    if parse_failed
        || rust_failed
        || reports.values().flatten().any(|r| r.code.severity().is_error())
    {
        output.diagnostics = resolve_reports(reports, &paths, &sources);
        output.diagnostics.append(&mut rust_diagnostics);
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

    let crates: Vec<crate::CrateIr> = project.iter().map(|project| project.ir.clone()).collect();
    let mut analysis = analyze::run(&modules, &output.routes, options, &crates);
    for (module, mut module_reports) in
        std::mem::take(&mut analysis.reports).into_iter().enumerate()
    {
        reports.entry(module).or_default().append(&mut module_reports);
    }
    let analysis_failed = reports.values().flatten().any(|r| r.code.severity().is_error());
    let mut plan = None;
    if !analysis_failed {
        let mut client = client::plan(&modules, &analysis, &output.routes, options.chunks);
        for (module, mut module_reports) in
            std::mem::take(&mut client.reports).into_iter().enumerate()
        {
            reports.entry(module).or_default().append(&mut module_reports);
        }
        plan = Some(client);
    }
    let failed = reports.values().flatten().any(|r| r.code.severity().is_error());
    if let (Some(plan), Some(project), Some(rust_output), false) =
        (&plan, &project, &mut output.rust, failed)
    {
        let (code, rust_reports) =
            rust::emit(&modules, &analysis, &output.routes, plan, project, options.dev);
        for (module, mut module_reports) in rust_reports.into_iter().enumerate() {
            if !module_reports.is_empty() {
                reports.entry(module).or_default().append(&mut module_reports);
            }
        }
        rust_output.main = Some(code);
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
    output.app = emit::app(&modules, &analysis, &output.routes, &plan.routes);
    output.navigation = emit::navigation(&modules, &analysis, &output.routes);
    output.chunks = std::mem::take(&mut plan.chunks);
    output
}

fn exported_names<'a>(program: &Program<'a>) -> Vec<(&'a str, Span)> {
    let mut out = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                            out.push((id.name.as_str(), id.span));
                        }
                    }
                }
                Declaration::FunctionDeclaration(function) => {
                    if let Some(id) = &function.id {
                        out.push((id.name.as_str(), id.span));
                    }
                }
                _ => {}
            },
            Statement::ExportNamedDeclaration(export) => {
                for specifier in &export.specifiers {
                    out.push((specifier.exported.name().as_str(), specifier.span));
                }
            }
            _ => {}
        }
    }
    out
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
