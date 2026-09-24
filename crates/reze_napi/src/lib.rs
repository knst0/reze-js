use std::collections::HashMap;

use napi_derive::napi;

#[napi(object)]
pub struct CompileOptions {
    /// Module the generated code imports its runtime from. Default: `"reze-js"`.
    pub module_name: Option<String>,
    /// Default: `true`.
    pub source_map: Option<bool>,
    /// Constant signals and dead JSX branches (O3, O5). Default: `true`.
    pub optimize: Option<bool>,
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

/// Compiles the JSX in `source` to DOM code. Returns `null` when the file has no JSX.
/// Compile errors are returned as `error` diagnostics with `code: null`, never thrown.
#[napi]
pub fn compile(
    source: String,
    filename: String,
    options: Option<CompileOptions>,
) -> Option<CompileResult> {
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
    }
    match reze_compiler::compile(&source, &filename, &opts) {
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
    }
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
