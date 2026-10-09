use std::collections::HashMap;

use napi_derive::napi;

#[napi(object)]
pub struct ProfileComponentFacts {
    pub component: String,
    pub file: String,
    pub mounts: u32,
    pub props: u32,
    pub reruns: u32,
    pub writes: u32,
}

#[napi(object)]
pub struct ProfileFacts {
    /// Session-tree schema version; anything but the compiler's version is ignored.
    pub v: u32,
    /// The compiled file, as passed for `filename`.
    pub file: String,
    /// FNV-1a64 of the compiled source, hex; a mismatch compiles as without facts.
    pub hash: String,
    pub components: Vec<ProfileComponentFacts>,
}

#[napi(string_enum = "lowercase")]
pub enum CompileTarget {
    Client,
    Island,
    Html,
}

#[napi(object)]
pub struct CompileOptions {
    /// Default: `true`.
    pub source_map: Option<bool>,
    /// Pass `{ name }` to `signal`/`computed`/`action` for profiling. Default: `false`.
    pub debug_names: Option<bool>,
    /// Register components for hot-swap through `import.meta.hot`. Default: `false`.
    pub hot: Option<bool>,
    /// Module exporting `link`: native `<a href>` elements are claimed and passed to it. Default: none.
    pub links: Option<String>,
    /// Which output to produce. Default: `"client"`.
    pub target: Option<CompileTarget>,
    /// Stable canonical module id; required (nonempty) for `island` and `html`. Default: none.
    pub module_id: Option<String>,
    /// Profiling facts for this file, from the profile store. Default: none.
    pub profile: Option<ProfileFacts>,
    /// Reactive facts of imported modules keyed by import specifier, from `analyze`. Each entry
    /// must be for the exact source the import resolves to. Default: none.
    pub facts: Option<serde_json::Value>,
}

#[napi(object)]
pub struct Position {
    /// Byte offset.
    pub offset: u32,
    /// 1-based.
    pub line: u32,
    /// 0-based, in UTF-16 code units.
    pub column: u32,
}

#[napi(object)]
pub struct Label {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// Replaces source bytes `start..end` with `text`.
#[napi(object)]
pub struct Edit {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// Edits that, applied together, remove the diagnostic.
#[napi(object)]
pub struct Fix {
    pub title: String,
    pub edits: Vec<Edit>,
}

#[napi(object)]
pub struct Diagnostic {
    /// Stable code; see `skills/reze-compiler-diagnostics/SKILL.md`.
    pub code: String,
    #[napi(ts_type = "\"error\" | \"warn\" | \"info\"")]
    pub severity: String,
    /// `[CODE] …`.
    pub message: String,
    pub file: String,
    pub start: Position,
    pub end: Position,
    /// Enclosing components (`<Name>`) and elements, root first.
    pub path: Vec<String>,
    pub labels: Vec<Label>,
    pub fixes: Vec<Fix>,
    pub data: HashMap<String, String>,
    /// Repair guide URL anchored at the code.
    pub docs: String,
    /// Message, `in` path, location, code frame and fixes.
    pub rendered: String,
}

#[napi(object)]
pub struct CompileResult {
    /// Absent when `diagnostics` holds an `error`.
    pub code: Option<String>,
    /// Source map v3 JSON.
    pub map: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Reactive facts of this module's exports; absent when `diagnostics` holds an `error`.
    pub facts: Option<serde_json::Value>,
}

fn position(position: reze_compiler::Position) -> Position {
    Position { offset: position.offset, line: position.line, column: position.column }
}

fn diagnostic(d: reze_compiler::Diagnostic) -> Diagnostic {
    Diagnostic {
        code: d.code.name().to_string(),
        severity: d.severity.as_str().to_string(),
        message: d.message,
        file: d.file,
        start: position(d.start),
        end: position(d.end),
        path: d.path,
        labels: d
            .labels
            .into_iter()
            .map(|l| Label { start: l.start, end: l.end, message: l.message })
            .collect(),
        fixes: d
            .fixes
            .into_iter()
            .map(|fix| Fix {
                title: fix.title,
                edits: fix
                    .edits
                    .into_iter()
                    .map(|e| Edit { start: e.start, end: e.end, text: e.text })
                    .collect(),
            })
            .collect(),
        data: d.data.into_iter().collect(),
        docs: d.docs,
        rendered: d.rendered,
    }
}

#[napi(object)]
pub struct AnalyzeResult {
    /// Absent when `diagnostics` holds an `error`.
    pub facts: Option<serde_json::Value>,
    pub diagnostics: Vec<Diagnostic>,
}

fn facts_value(facts: &reze_compiler::ModuleFacts) -> napi::Result<serde_json::Value> {
    serde_json::to_value(facts).map_err(|error| napi::Error::from_reason(error.to_string()))
}

fn diagnostics(diagnostics: Vec<reze_compiler::Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.into_iter().map(diagnostic).collect()
}

/// Compiles `source`. `code` is absent when nothing in the file is rewritten, and when
/// `diagnostics` holds an `error`; compile errors come back as diagnostics without `code`, and the
/// call never throws on them.
#[napi]
pub fn compile(
    source: String,
    filename: String,
    options: Option<CompileOptions>,
) -> napi::Result<CompileResult> {
    let mut opts = reze_compiler::Options::default();
    if let Some(o) = options {
        opts.source_map = o.source_map.unwrap_or(opts.source_map);
        opts.debug_names = o.debug_names.unwrap_or(opts.debug_names);
        opts.hot = o.hot.unwrap_or(opts.hot);
        opts.links = o.links.or(opts.links);
        opts.target = match o.target {
            Some(CompileTarget::Island) => reze_compiler::CompileTarget::Island,
            Some(CompileTarget::Html) => reze_compiler::CompileTarget::Html,
            Some(CompileTarget::Client) | None => reze_compiler::CompileTarget::Client,
        };
        opts.module_id = o.module_id;
        opts.profile = o.profile.map(|facts| reze_compiler::ProfileFacts {
            v: facts.v,
            file: facts.file,
            hash: facts.hash,
            components: facts
                .components
                .into_iter()
                .map(|c| reze_compiler::ProfileComponent {
                    component: c.component,
                    file: c.file,
                    mounts: c.mounts,
                    props: c.props,
                    reruns: c.reruns,
                    writes: c.writes,
                })
                .collect(),
        });
        opts.facts = match o.facts {
            Some(value) => serde_json::from_value(value)
                .map_err(|error| napi::Error::from_reason(format!("invalid facts: {error}")))?,
            None => HashMap::new(),
        };
    }
    match reze_compiler::compile_with_facts(&source, &filename, &opts) {
        Ok(reze_compiler::Compiled { output, facts }) => {
            let (code, map, diagnostics_out) = match output {
                Some(out) => (Some(out.code), out.map, out.diagnostics),
                None => (None, None, Vec::new()),
            };
            Ok(CompileResult {
                code,
                map,
                diagnostics: diagnostics(diagnostics_out),
                facts: Some(facts_value(&facts)?),
            })
        }
        Err(errors) => Ok(CompileResult {
            code: None,
            map: None,
            diagnostics: diagnostics(errors),
            facts: None,
        }),
    }
}

/// Reactive facts of `source`'s exports, for `compile`'s `facts` of imported modules. `facts` is
/// absent when the module has errors.
#[napi]
pub fn analyze(source: String, filename: String) -> napi::Result<AnalyzeResult> {
    match reze_compiler::analyze(&source, &filename) {
        Ok(facts) => {
            Ok(AnalyzeResult { facts: Some(facts_value(&facts)?), diagnostics: Vec::new() })
        }
        Err(errors) => Ok(AnalyzeResult { facts: None, diagnostics: diagnostics(errors) }),
    }
}
