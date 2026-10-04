//! JSX and reactive syntax compiled through shared semantic IR into target-specific Oxc AST.

mod ast;
mod codegen;
mod diagnostic;
mod frontend;
mod html;
mod imports;
mod ir;
mod kind;

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

pub use diagnostic::{
    CATALOG, Code, Diagnostic, Edit, Entry, Example, Fix, Label, Position, Severity, render_skill,
};

use diagnostic::Report;

/// The module compiled code imports runtime helpers from.
pub const RUNTIME_MODULE: &str = "reze-js";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompileTarget {
    #[default]
    Client,
    Hydrate,
    Html,
}

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
    /// Default: `Client`.
    pub target: CompileTarget,
    /// Nonempty canonical identity required by `Hydrate` and `Html`.
    pub module_id: Option<String>,
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
            target: CompileTarget::Client,
            module_id: None,
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
}

/// Compiles `source`. `Ok(None)` when nothing in the file is rewritten; `Err` holds every
/// diagnostic when at least one is an `error`. `filename` picks the dialect (unknown extensions
/// parse as TSX) and names the source in diagnostics, source maps and hot-swap ids.
/// `Hydrate` and `Html` require a nonempty `Options.module_id`; missing identity reports
/// `MISSING_MODULE_ID` before parsing.
pub fn compile(
    source: &str,
    filename: &str,
    options: &Options,
) -> Result<Option<Output>, Vec<Diagnostic>> {
    if matches!(options.target, CompileTarget::Hydrate | CompileTarget::Html)
        && options.module_id.as_deref().is_none_or(str::is_empty)
    {
        let target = match options.target {
            CompileTarget::Hydrate => "hydrate",
            CompileTarget::Html => "html",
            CompileTarget::Client => "client",
        };
        let reports =
            vec![Report::new(Code::MissingModuleId, Span::empty(0)).arg("target", target)];
        return Err(diagnostic::resolve(reports, source, filename));
    }
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    compile_module(source, filename, source_type, options)
}

fn compile_module(
    source: &str,
    filename: &str,
    source_type: SourceType,
    options: &Options,
) -> Result<Option<Output>, Vec<Diagnostic>> {
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
                Report::new(Code::ParseError, span).arg("detail", d.message.to_string())
            })
            .collect();
        return Err(diagnostic::resolve(reports, source, filename));
    }

    let program = allocator.alloc(parsed.program);
    let hot_plan = codegen::hot::should_apply(options).then(|| codegen::hot::collect(program));
    let module_id = (options.target != CompileTarget::Client).then_some(options.module_id.as_deref()).flatten();
    let sites = ir::collect_sites(program, module_id, source);
    let normalized = frontend::normalize(&allocator, program, source);
    let mut module = ir::build_module_ir(
        normalized.program, &normalized.scoping, &normalized.facts,
        source, module_id, options.links.is_some(), sites,
    );
    for view in &mut module.views {
        if let ir::view::ViewKind::Element(element) = &mut view.kind {
            ir::layout::normalize(element, view.origin, &mut module.reports);
        }
    }
    let mut reports = normalized.reports;
    reports.append(&mut module.reports);
    let diagnostics = diagnostic::resolve(reports, source, filename);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(diagnostics);
    }
    if !module.has_views && !normalized.content_changed && normalized.facts.folded_bindings.is_empty()
        && normalized.facts.dynamic_tags.is_empty()
        && !(options.debug_names && options.target != CompileTarget::Html)
        && !(options.target != CompileTarget::Client && !normalized.facts.runtime_calls.is_empty())
    {
        return Ok(None);
    }
    let cold = options.target != CompileTarget::Html
        && is_cold(source, filename, options.profile.as_ref());
    let changed = codegen::EmitContext::new(
        &allocator, options, source, filename, &module, &normalized.facts, &normalized.scoping,
        normalized.namer, normalized.helpers, cold,
    ).emit(normalized.program, hot_plan.as_ref());
    if !changed && !normalized.content_changed {
        return Ok(None);
    }
    let output = Codegen::new().with_options(CodegenOptions {
        source_map_path: options.source_map.then(|| filename.into()),
        ..CodegenOptions::default()
    }).build(normalized.program);
    Ok(Some(Output {
        code: output.code,
        map: output.map.map(codegen::serialize_source_map),
        diagnostics,
    }))
}
