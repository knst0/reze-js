//! Reze compiler: JSX → DOM code for the client, HTML strings for the server, or DOM claiming
//! for hydration, with diagnostics, module-level optimizations and whole-program analysis. The
//! contract is `crates/reze_compiler/SPEC.md`.

mod analyze;
mod code;
pub mod diagnostic;
mod emit;
pub mod facts;
pub mod features;
mod html;
mod ir;
mod link;
mod lower;
mod namer;
pub mod summary;
mod usage;
mod verify;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

pub use diagnostic::{Code, Diagnostic, Edit, Fix, Label, Position, Related, Severity};
pub use facts::{ModuleFacts, Reason};
pub use features::Features;
pub use link::{LinkOptions, Linked, ModuleInput, link};
pub use summary::{ModuleSummary, SummaryOptions, summarize};
pub use verify::{OutsideModule, verify};

use diagnostic::Report;
use namer::Namer;

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
    /// Enables O3 (constant signals), O4 (inlined computeds), O5 (dead JSX branches) and store
    /// unproxying; O1 and O2 always run.
    pub optimize: bool,
    pub target: Target,
    /// Names `signal` and `computed` nodes after the variables they are declared into, for
    /// devtools; meant for dev builds only.
    pub debug_names: bool,
    /// Program decisions from `link` (SPEC §15); `None` compiles the module on its own.
    pub facts: Option<ModuleFacts>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            module_name: "reze-js".to_string(),
            source_map: true,
            optimize: true,
            target: Target::Client,
            debug_names: false,
            facts: None,
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

/// Compiles `source`. `Ok(None)` when nothing in the file is rewritten (no JSX, no fold); `Err`
/// holds every diagnostic when at least one is an `error`. `filename` picks the dialect and names
/// the source in diagnostics and the source map.
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

    if let Some(facts) = &options.facts
        && facts.source_hash != facts::source_hash(source)
    {
        let report = Report::new(
            Code::FactsStale,
            Span::empty(0),
            "The program facts were built for a different version of this module: another plugin \
             changed it between the program scan and `transform`. Order that plugin after Reze, \
             exclude the module from `program.include`, or disable `optimize`.",
        );
        return Err(diagnostic::resolve(vec![report], source, filename));
    }

    let program = allocator.alloc(parsed.program);
    let (scoping, nodes) = SemanticBuilder::new()
        .with_build_nodes(true)
        .build(program)
        .semantic
        .into_scoping_and_nodes();
    let mut reports = Vec::new();
    let facts = analyze::analyze(
        program,
        &scoping,
        &nodes,
        &options.module_name,
        options.optimize,
        options.facts.as_ref(),
        &mut reports,
    );
    let header_at = header_position(program);
    let lowerer = lower::Lowerer::new(
        &allocator,
        source,
        &facts,
        &scoping,
        &nodes,
        options.optimize,
        options.debug_names,
        Namer::new(&scoping),
        reports,
    );
    let lowered = lowerer.program(program, header_at);
    let Some(body) = lowered.body else { return Ok(None) };

    let diagnostics = diagnostic::resolve(lowered.reports, source, filename);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(diagnostics);
    }
    let emitter = emit::Emitter::new(
        &allocator,
        source,
        &options.module_name,
        source_type.is_typescript(),
        options.target,
        lowered.namer,
    );
    let code = emitter.module(&lowered.head, &body);
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
