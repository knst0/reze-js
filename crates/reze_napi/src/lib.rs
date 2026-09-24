use napi_derive::napi;

#[napi(object)]
pub struct CompileOptions {
    /// Module the generated code imports its runtime from. Default: `"reze-js"`.
    pub module_name: Option<String>,
    /// Default: `true`.
    pub source_map: Option<bool>,
}

#[napi(object)]
pub struct CompileWarning {
    pub message: String,
    /// 1-based.
    pub line: u32,
    /// 0-based, in UTF-16 code units.
    pub column: u32,
}

#[napi(object)]
pub struct CompileResult {
    pub code: String,
    /// Source map v3 JSON.
    pub map: Option<String>,
    /// Non-fatal diagnostics.
    pub warnings: Vec<CompileWarning>,
}

/// Compiles the JSX in `source` to DOM code. Returns `null` when the file has no JSX.
/// `filename` picks the dialect (`.tsx`, `.jsx`, …) and names the source in the map.
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
    }
    match reze_compiler::compile(&source, &filename, &opts) {
        Ok(out) => Ok(out.map(|o| CompileResult {
            code: o.code,
            map: o.map,
            warnings: o
                .warnings
                .into_iter()
                .map(|w| CompileWarning { message: w.message, line: w.line, column: w.column })
                .collect(),
        })),
        Err(errors) => {
            let message: Vec<String> = errors.iter().map(|e| format!("{filename}:{e}")).collect();
            Err(napi::Error::from_reason(message.join("\n")))
        }
    }
}
