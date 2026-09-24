//! Reze compiler: JSX → DOM code for the client, HTML strings for the server, or DOM claiming
//! for hydration, with diagnostics and module-level optimizations. The contract is
//! `crates/reze_compiler/SPEC.md`.

mod analyze;
mod code;
pub mod diagnostic;
mod emit;
mod html;
mod ir;
mod lower;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

pub use diagnostic::{Code, Diagnostic, Edit, Fix, Label, Position, Severity};

use diagnostic::Report;

/// What the compiled module does with JSX.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Target {
    /// Clones templates and builds the DOM.
    #[default]
    Client,
    /// Concatenates HTML strings, with hydration keys and insert markers.
    Server,
    /// Claims the DOM the `Server` output rendered, cloning what it cannot claim.
    Hydrate,
}

pub struct Options {
    /// Module the generated code imports its runtime helpers from.
    pub module_name: String,
    pub source_map: bool,
    /// Enables O3 (constant signals) and O5 (dead JSX branches); O1 and O2 always run.
    pub optimize: bool,
    pub target: Target,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            module_name: "reze-js".to_string(),
            source_map: true,
            optimize: true,
            target: Target::Client,
        }
    }
}

pub struct Output {
    pub code: String,
    /// Source map v3 JSON.
    pub map: Option<String>,
    /// `warn` and `info` diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// Compiles the JSX in `source`. `Ok(None)` when the file has no JSX; `Err` holds every
/// diagnostic when at least one is an `error`. `filename` picks the dialect and names the source
/// in diagnostics and the source map.
pub fn compile(
    source: &str,
    filename: &str,
    options: &Options,
) -> Result<Option<Output>, Vec<Diagnostic>> {
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        let reports = parsed
            .diagnostics
            .iter()
            .map(|d| {
                let span = d.labels.first().map_or(Span::empty(0), |label| {
                    let start = label.offset();
                    Span::new(start, start + label.len())
                });
                Report::new(Code::ParseError, span, d.message.to_string())
            })
            .collect();
        return Err(diagnostic::resolve(reports, source, filename));
    }

    let program = allocator.alloc(parsed.program);
    let scoping = SemanticBuilder::new().build(program).semantic.into_scoping();
    let mut reports = Vec::new();
    let facts =
        analyze::analyze(program, &scoping, &options.module_name, options.optimize, &mut reports);
    let header_at = header_position(program);
    let lowerer =
        lower::Lowerer::new(&allocator, source, &facts, &scoping, options.optimize, &mut reports);
    let Some(body) = lowerer.program(program, header_at) else { return Ok(None) };

    let diagnostics = diagnostic::resolve(reports, source, filename);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(diagnostics);
    }
    let emitter = emit::Emitter::new(
        &allocator,
        source,
        &options.module_name,
        source_type.is_typescript(),
        options.target,
        &scoping,
    );
    let code = emitter.module(&body, header_at);
    let map = options.source_map.then(|| code.source_map(filename, source));
    Ok(Some(Output { code: code.text, map, diagnostics }))
}

/// Runtime imports and templates go after the hashbang, directives and leading imports.
fn header_position(program: &Program<'_>) -> u32 {
    let mut at = program.hashbang.as_ref().map_or(0, |h| h.span.end);
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

pub use diagnostic::catalog::render_skill;
