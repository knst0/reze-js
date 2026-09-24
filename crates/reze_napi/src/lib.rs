use std::collections::HashMap;

use napi_derive::napi;

#[napi(object)]
pub struct CompileOptions {
    /// Module the generated code imports its runtime from. Default: `"reze-js"`.
    pub module_name: Option<String>,
    /// Default: `true`.
    pub source_map: Option<bool>,
    /// Constant signals, inlined computeds, dead JSX branches and store unproxying. Default: `true`.
    pub optimize: Option<bool>,
    /// `"client"` builds the DOM, `"server"` renders HTML strings for `renderToString`,
    /// `"hydrate"` claims that HTML in the browser. Default: `"client"`.
    #[napi(ts_type = "\"client\" | \"server\" | \"hydrate\"")]
    pub target: Option<String>,
    /// This module's facts from `link`, as returned there.
    pub facts: Option<String>,
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

/// A span in another module; byte offsets only.
#[napi(object)]
pub struct Related {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// Replaces bytes `start..end` of the source with `text`.
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
    /// Stable `SCREAMING_SNAKE` code; see `skills/compiler-diagnostics/SKILL.md`.
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
    /// The rest of a cross-module reason chain.
    pub related: Vec<Related>,
    pub fixes: Vec<Fix>,
    pub data: HashMap<String, String>,
    /// Repair guide URL anchored at the code.
    pub docs: String,
    /// Message, `in` line, location, code frame and fixes, without the once-per-code footer.
    pub rendered: String,
}

#[napi(object)]
pub struct CompileResult {
    /// `null` when `diagnostics` holds an `error`.
    pub code: Option<String>,
    /// Source map v3 JSON.
    pub map: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

fn invalid(message: String) -> napi::Error {
    napi::Error::from_reason(message)
}

/// Parses an internal JSON payload of this crate version; another version throws.
fn parse_versioned<T: serde::de::DeserializeOwned>(json: &str, what: &str) -> napi::Result<T> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| invalid(format!("invalid {what}: {e}")))?;
    let version = value.get("version").and_then(|v| v.as_str()).unwrap_or_default();
    if version != reze_compiler::facts::VERSION {
        return Err(invalid(format!(
            "{what} was built by @rezejs/compiler {version}, this is {}: rebuild it",
            reze_compiler::facts::VERSION
        )));
    }
    serde_json::from_value(value).map_err(|e| invalid(format!("invalid {what}: {e}")))
}

/// Compiles `source`. Returns `null` when nothing in the file is rewritten.
/// Compile errors are returned as `error` diagnostics without `code`; only invalid options
/// throw.
#[napi]
pub fn compile(
    source: String,
    filename: String,
    options: Option<CompileOptions>,
) -> napi::Result<Option<CompileResult>> {
    let mut opts = reze_compiler::Options::default();
    if let Some(o) = options {
        if let Some(module_name) = o.module_name {
            opts.module_name = module_name;
        }
        if let Some(source_map) = o.source_map {
            opts.source_map = source_map;
        }
        if let Some(optimize) = o.optimize {
            opts.optimize = optimize;
        }
        if let Some(target) = o.target {
            opts.target = match target.as_str() {
                "client" => reze_compiler::Target::Client,
                "server" => reze_compiler::Target::Server,
                "hydrate" => reze_compiler::Target::Hydrate,
                other => {
                    return Err(invalid(format!(
                        "unknown target `{other}`: expected \"client\", \"server\" or \"hydrate\""
                    )));
                }
            };
        }
        if let Some(facts) = o.facts {
            opts.facts = Some(parse_versioned(&facts, "facts")?);
        }
    }
    Ok(match reze_compiler::compile(&source, &filename, &opts) {
        Ok(None) => None,
        Ok(Some(out)) => Some(CompileResult {
            code: Some(out.code),
            map: out.map,
            diagnostics: out.diagnostics.into_iter().map(diagnostic).collect(),
        }),
        Err(errors) => Some(CompileResult {
            code: None,
            map: None,
            diagnostics: errors.into_iter().map(diagnostic).collect(),
        }),
    })
}

#[napi(object)]
pub struct SummarizeOptions {
    /// Default: `"reze-js"`.
    pub module_name: Option<String>,
}

#[napi(object)]
pub struct SummarizeResult {
    /// Opaque JSON for `link`; missing when `diagnostics` holds a parse error.
    pub summary: Option<String>,
    /// Import specifiers to resolve for `link`'s `resolved`, in the summary's order.
    pub specifiers: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

/// What `link` needs to know about one module of the program. Any dialect, JSX or not.
#[napi]
pub fn summarize(
    source: String,
    filename: String,
    options: Option<SummarizeOptions>,
) -> napi::Result<SummarizeResult> {
    let mut opts = reze_compiler::SummaryOptions::default();
    if let Some(module_name) = options.and_then(|o| o.module_name) {
        opts.module_name = module_name;
    }
    Ok(match reze_compiler::summarize(&source, &filename, &opts) {
        Ok(summary) => SummarizeResult {
            summary: Some(serde_json::to_string(&summary).map_err(|e| invalid(e.to_string()))?),
            specifiers: summary.specifiers,
            diagnostics: Vec::new(),
        },
        Err(errors) => SummarizeResult {
            summary: None,
            specifiers: Vec::new(),
            diagnostics: errors.into_iter().map(diagnostic).collect(),
        },
    })
}

#[napi(object)]
pub struct LinkModule {
    pub id: String,
    /// From `summarize`.
    pub summary: String,
    /// Program module id per specifier of the summary, `null` outside the program.
    pub resolved: Vec<Option<String>>,
    pub is_entry: bool,
}

#[napi(object)]
pub struct LinkOptions {
    /// Default: `true`.
    pub optimize: Option<bool>,
    /// Default: `false`.
    pub islands: Option<bool>,
    /// Directory island ids are relative to (Vite `config.root`). Default: `""`.
    pub root: Option<String>,
}

#[napi(object)]
pub struct LinkResult {
    /// Opaque JSON per module id, for `compile({ facts })`.
    pub facts: HashMap<String, String>,
    /// Program-decided define flags by name.
    pub features: HashMap<String, bool>,
    /// Modules no module outside the program may import.
    pub closed: Vec<String>,
}

/// Whole-program decisions: folds across modules, static components, islands, feature flags.
#[napi]
pub fn link(modules: Vec<LinkModule>, options: Option<LinkOptions>) -> napi::Result<LinkResult> {
    let options = options.unwrap_or(LinkOptions { optimize: None, islands: None, root: None });
    let inputs = modules
        .into_iter()
        .map(|m| {
            Ok(reze_compiler::ModuleInput {
                summary: parse_versioned(&m.summary, "summary")?,
                id: m.id,
                resolved: m.resolved,
                is_entry: m.is_entry,
            })
        })
        .collect::<napi::Result<Vec<_>>>()?;
    let linked = reze_compiler::link(
        &inputs,
        &reze_compiler::LinkOptions {
            optimize: options.optimize.unwrap_or(true),
            islands: options.islands.unwrap_or(false),
            root: options.root.unwrap_or_default(),
        },
    );
    let facts = linked
        .facts
        .into_iter()
        .map(|(id, facts)| {
            Ok((id, serde_json::to_string(&facts).map_err(|e| invalid(e.to_string()))?))
        })
        .collect::<napi::Result<_>>()?;
    Ok(LinkResult { facts, features: linked.features.into_iter().collect(), closed: linked.closed })
}

#[napi(object)]
pub struct OutsideModule {
    pub id: String,
    /// From `summarize`, for modules that mention a runtime module; otherwise `null`.
    pub summary: Option<String>,
    /// Ids of every module it imports, statically or dynamically.
    pub imported: Vec<String>,
}

/// Closed-world and feature-flag errors for modules outside the program.
#[napi]
pub fn verify(linked: LinkResult, outside: Vec<OutsideModule>) -> napi::Result<Vec<Diagnostic>> {
    let linked = reze_compiler::Linked {
        facts: HashMap::new(),
        features: linked.features.into_iter().collect(),
        closed: linked.closed,
    };
    let outside = outside
        .into_iter()
        .map(|m| {
            Ok(reze_compiler::OutsideModule {
                summary: m.summary.as_deref().map(|s| parse_versioned(s, "summary")).transpose()?,
                id: m.id,
                imported: m.imported,
            })
        })
        .collect::<napi::Result<Vec<_>>>()?;
    Ok(reze_compiler::verify(&linked, &outside).into_iter().map(diagnostic).collect())
}

fn diagnostic(d: reze_compiler::Diagnostic) -> Diagnostic {
    let position =
        |p: reze_compiler::Position| Position { offset: p.offset, line: p.line, column: p.column };
    Diagnostic {
        code: d.code.name().to_string(),
        severity: d.severity.as_str().to_string(),
        docs: d.docs(),
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
        related: d
            .related
            .into_iter()
            .map(|r| Related { file: r.file, start: r.start, end: r.end, message: r.message })
            .collect(),
        fixes: d
            .fixes
            .into_iter()
            .map(|f| Fix {
                title: f.title,
                edits: f
                    .edits
                    .into_iter()
                    .map(|e| Edit { start: e.start, end: e.end, text: e.text })
                    .collect(),
            })
            .collect(),
        data: d.data.into_iter().collect(),
        rendered: d.rendered,
    }
}
