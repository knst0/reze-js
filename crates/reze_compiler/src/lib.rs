//! JSX → DOM programs for the browser, with diagnostics from one catalog.

mod analyze;
mod code;
mod diagnostic;
mod dsl;
mod emit;
mod html;
mod ir;
mod kind;
mod lower;
mod namer;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind, Program, Statement};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

pub use diagnostic::{
    CATALOG, Code, Diagnostic, Edit, Entry, Example, Fix, Label, Position, Severity, render_skill,
};
pub use lower::{ComponentRef, PrerenderComponent, PrerenderHole, PrerenderModule, Tree};

use diagnostic::Report;
use namer::Namer;

/// The module compiled code imports runtime helpers from.
pub const RUNTIME_MODULE: &str = "reze-js";

pub struct Options {
    /// Emit a v3 source map.
    pub source_map: bool,
    /// Pass `{ name }` to `signal`/`computed`/`action` after the declared variable, for profiling.
    pub debug_names: bool,
    /// Register components for hot-swap through `import.meta.hot`.
    pub hot: bool,
    /// Module exporting `link`: when set, native `<a href>` elements are claimed and passed to
    /// `link(el, href?)`, which keeps their `aria-current`/`data-active`/`data-pending` current.
    pub links: Option<String>,
    /// Collect static prerender trees alongside codegen.
    pub prerender: bool,
    /// Profiling record for this file, in the session-tree shape the host stores. The file is
    /// specialized only when the record names this file with a matching schema and source hash;
    /// anything else compiles as without facts.
    pub profile: Option<ProfileFacts>,
}

/// Schema version of `ProfileFacts`; records with another version are ignored.
pub const PROFILE_VERSION: u32 = 1;

/// One dev session's counters for a component, as collected by `startProfileSession`.
#[derive(Default, serde::Deserialize)]
pub struct ProfileComponent {
    pub component: String,
    pub file: String,
    pub mounts: u32,
    pub props: u32,
    pub reruns: u32,
    pub writes: u32,
}

/// Profiling record for one file: `hash` is FNV-1a64 of the compiled source, hex.
#[derive(Default, serde::Deserialize)]
pub struct ProfileFacts {
    pub v: u32,
    pub file: String,
    pub hash: String,
    pub components: Vec<ProfileComponent>,
}

/// FNV-1a64 of `text`, lowercase hex. The dev server computes the same hash to key the store.
pub fn profile_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Whether `profile` describes `filename` and `source` with at least one mount and zero re-runs.
fn is_cold(source: &str, filename: &str, profile: Option<&ProfileFacts>) -> bool {
    let Some(facts) = profile else { return false };
    if facts.v != PROFILE_VERSION
        || facts.file != filename
        || facts.hash != profile_hash(source)
        || facts.components.is_empty()
    {
        return false;
    }
    facts.components.iter().any(|c| c.mounts > 0) && facts.components.iter().all(|c| c.reruns == 0)
}

impl Default for Options {
    fn default() -> Self {
        Self {
            source_map: true,
            debug_names: false,
            hot: false,
            links: None,
            prerender: false,
            profile: None,
        }
    }
}

pub struct Output {
    pub code: String,
    /// Source map v3 JSON.
    pub map: Option<String>,
    /// `warn` and `info` diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Static prerender trees, when `Options.prerender` is set.
    pub prerender: Option<PrerenderModule>,
}

/// Compiles `source`. `Ok(None)` when nothing in the file is rewritten; `Err` holds every
/// diagnostic when at least one is an `error`. `filename` picks the dialect (unknown extensions
/// parse as TSX) and names the source in diagnostics, source maps and hot-swap ids.
pub fn compile(
    source: &str,
    filename: &str,
    options: &Options,
) -> Result<Option<Output>, Vec<Diagnostic>> {
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    let (rewritten, first_pass) = if dsl::mentions_syntax(source) {
        match dsl::rewrite(source, source_type) {
            Ok(Some((rewritten, reports))) => (Some(rewritten), reports),
            Ok(None) => (None, Vec::new()),
            Err(reports) => return Err(diagnostic::resolve(reports, source, filename)),
        }
    } else {
        (None, Vec::new())
    };
    compile_module(source, rewritten, first_pass, filename, source_type, options)
}

/// The ordinary pass over `source`, or over the text the first pass rewrote it into.
fn compile_module(
    source: &str,
    rewritten: Option<dsl::Rewritten>,
    first_pass: Vec<Report>,
    filename: &str,
    source_type: SourceType,
    options: &Options,
) -> Result<Option<Output>, Vec<Diagnostic>> {
    let text = rewritten.as_ref().map_or(source, |r| r.code.text.as_str());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, text, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        let reports = parsed
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.labels.first().map_or(Span::empty(0), |label| {
                    let start = label.offset();
                    Span::new(start, start + label.len())
                });
                Report::new(Code::ParseError, span).arg("detail", d.message.to_string())
            })
            .collect();
        return Err(diagnostic::resolve(reports, text, filename));
    }

    let program = allocator.alloc(parsed.program);
    let (scoping, nodes) = SemanticBuilder::new()
        .with_build_nodes(true)
        .build(program)
        .semantic
        .into_scoping_and_nodes();
    let mut reports = Vec::new();
    let analysis = analyze::analyze(program, &scoping, &nodes, &mut reports);
    let settings = lower::Settings {
        debug_names: options.debug_names,
        hot: options.hot,
        links: options.links.is_some(),
    };
    let lowerer =
        lower::Lowerer::new(&allocator, text, &analysis, settings, Namer::new(&scoping), reports);
    let lowered = lowerer.program(program, header_position(program));
    let mut reports = lowered.reports;
    if let Some(rewritten) = &rewritten {
        for report in &mut reports {
            report.remap(|offset| rewritten.start(offset), |offset| rewritten.end(offset));
        }
    }
    reports.extend(first_pass);
    let diagnostics = diagnostic::resolve(reports, source, filename);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(diagnostics);
    }
    let prerender = options
        .prerender
        .then(|| {
            let mut roots = lower::Lowerer::new(
                &allocator,
                text,
                &analysis,
                lower::Settings { debug_names: false, hot: false, links: options.links.is_some() },
                Namer::new(&scoping),
                Vec::new(),
            );
            roots.prerender_module(program)
        })
        .filter(|module| !module.is_empty());
    let filename = allocator.alloc_str(filename);
    let Some(body) = lowered.body else {
        return Ok(rewritten.map(|rewritten| Output {
            map: options.source_map.then(|| rewritten.code.source_map(filename, source)),
            code: rewritten.code.text,
            diagnostics,
            prerender,
        }));
    };
    let mut code = emit::Emitter::new(
        &allocator,
        text,
        filename,
        lowered.namer,
        options.links.as_deref(),
        emit::Options {
            debug_names: options.debug_names,
            cold: is_cold(source, filename, options.profile.as_ref()),
            hoist_templates: !program.body.iter().any(|statement| match statement {
                Statement::FunctionDeclaration(_) => true,
                Statement::ExportDeclaration(export) => {
                    matches!(export.declaration, Declaration::FunctionDeclaration(_))
                }
                Statement::ExportDefaultDeclaration(export) => matches!(
                    export.declaration,
                    ExportDefaultDeclarationKind::FunctionDeclaration(_)
                ),
                _ => false,
            }),
        },
    )
    .module(&lowered.head, &body);
    if let Some(rewritten) = &rewritten {
        code.remap_marks(|offset| rewritten.start(offset));
    }
    let map = options.source_map.then(|| code.source_map(filename, source));
    Ok(Some(Output { code: code.text, map, diagnostics, prerender }))
}

/// Runtime imports go after the hashbang, directives and leading imports.
fn header_position(program: &Program<'_>) -> u32 {
    let mut at = program.hashbang.as_ref().map_or(0, |hashbang| hashbang.span.end);
    if let Some(directive) = program.directives.last() {
        at = directive.span.end;
    }
    for statement in &program.body {
        match statement {
            Statement::ImportDeclaration(import) => at = import.span.end,
            _ => break,
        }
    }
    at.max(program.body.first().map_or(at, |s| s.span().start.min(at)))
}
