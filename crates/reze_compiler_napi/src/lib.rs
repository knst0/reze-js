use std::collections::HashMap;

use napi_derive::napi;

#[napi(object)]
pub struct CompileOptions {
    /// Default: `true`.
    pub source_map: Option<bool>,
    /// Pass `{ name }` to `signal`/`computed`/`action` for devtools. Default: `false`.
    pub debug_names: Option<bool>,
    /// Register components for hot-swap through `import.meta.hot`. Default: `false`.
    pub hot: Option<bool>,
    /// Module exporting `link`: native `<a href>` elements are claimed and passed to it. Default: none.
    pub links: Option<String>,
    /// Collect static prerender trees as JSON. Default: `false`.
    pub prerender: Option<bool>,
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
    /// Static prerender trees as JSON, when requested.
    pub prerender: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
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

/// Compiles `source`; `null` when nothing in the file is rewritten. Compile errors come back as
/// `error` diagnostics without `code`; the call never throws on them.
#[napi]
pub fn compile(
    source: String,
    filename: String,
    options: Option<CompileOptions>,
) -> Option<CompileResult> {
    let mut opts = reze_compiler::Options::default();
    if let Some(o) = options {
        opts.source_map = o.source_map.unwrap_or(opts.source_map);
        opts.debug_names = o.debug_names.unwrap_or(opts.debug_names);
        opts.hot = o.hot.unwrap_or(opts.hot);
        opts.links = o.links.or(opts.links);
        opts.prerender = o.prerender.unwrap_or(opts.prerender);
    }
    match reze_compiler::compile(&source, &filename, &opts) {
        Ok(None) => None,
        Ok(Some(out)) => Some(CompileResult {
            code: Some(out.code),
            map: out.map,
            diagnostics: out.diagnostics.into_iter().map(diagnostic).collect(),
            prerender: out
                .prerender
                .map(|module| serde_json::to_string(&module).unwrap_or_default()),
        }),
        Err(diagnostics) => Some(CompileResult {
            code: None,
            map: None,
            diagnostics: diagnostics.into_iter().map(diagnostic).collect(),
            prerender: None,
        }),
    }
}
