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
use oxc_ast::ast::{Program, Statement};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

pub use diagnostic::{
    CATALOG, Code, Diagnostic, Edit, Entry, Example, Fix, Label, Position, Severity, render_skill,
};

use diagnostic::Report;
use namer::Namer;

/// The module compiled code imports runtime helpers from.
pub const RUNTIME_MODULE: &str = "reze-js";

pub struct Options {
    /// Emit a v3 source map.
    pub source_map: bool,
    /// Pass `{ name }` to `signal`/`computed`/`action` after the declared variable, for devtools.
    pub debug_names: bool,
    /// Register components for hot-swap through `import.meta.hot`.
    pub hot: bool,
    /// Module exporting `link`: when set, native `<a href>` elements are claimed and passed to
    /// `link(el, href?)`, which keeps their `aria-current`/`data-active`/`data-pending` current.
    pub links: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self { source_map: true, debug_names: false, hot: false, links: None }
    }
}

pub struct Output {
    pub code: String,
    /// Source map v3 JSON.
    pub map: Option<String>,
    /// `warn` and `info` diagnostics.
    pub diagnostics: Vec<Diagnostic>,
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
    let filename = allocator.alloc_str(filename);
    let Some(body) = lowered.body else {
        return Ok(rewritten.map(|rewritten| Output {
            map: options.source_map.then(|| rewritten.code.source_map(filename, source)),
            code: rewritten.code.text,
            diagnostics,
        }));
    };
    let mut code =
        emit::Emitter::new(&allocator, text, filename, lowered.namer, options.links.as_deref())
            .module(&lowered.head, &body);
    if let Some(rewritten) = &rewritten {
        code.remap_marks(|offset| rewritten.start(offset));
    }
    let map = options.source_map.then(|| code.source_map(filename, source));
    Ok(Some(Output { code: code.text, map, diagnostics }))
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
