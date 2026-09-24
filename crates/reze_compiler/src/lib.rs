//! Reze compiler, v0: per-file JSX → DOM code generation for the client (ROADMAP M3).
//!
//! The output imports its runtime from `Options::module_name` and keeps all non-JSX source,
//! TypeScript included, verbatim.

mod code;
mod html;
mod transform;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

pub struct Options {
    /// Module the generated code imports its runtime helpers from.
    pub module_name: String,
    pub source_map: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { module_name: "reze-js".to_string(), source_map: true }
    }
}

pub struct Output {
    pub code: String,
    /// Source map v3 JSON.
    pub map: Option<String>,
    /// Non-fatal diagnostics (unknown props, ambiguous children, …).
    pub warnings: Vec<Warning>,
}

#[derive(Debug)]
pub struct Warning {
    pub message: String,
    /// 1-based.
    pub line: u32,
    /// 0-based, in UTF-16 code units.
    pub column: u32,
}

#[derive(Debug)]
pub struct Error {
    pub message: String,
    /// 1-based.
    pub line: u32,
    /// 0-based, in UTF-16 code units.
    pub column: u32,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column + 1, self.message)
    }
}

/// Compiles the JSX in `source`. `Ok(None)` when the file has no JSX.
/// `filename` picks the dialect (`.tsx`, `.jsx`, …) and names the source in the map.
pub fn compile(
    source: &str,
    filename: &str,
    options: &Options,
) -> Result<Option<Output>, Vec<Error>> {
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(parsed
            .diagnostics
            .iter()
            .map(|d| {
                let offset = d.labels.first().map_or(0, |l| l.offset() as usize);
                let (line, column) = position(source, offset);
                Error { message: d.message.to_string(), line, column }
            })
            .collect());
    }

    let transformer = transform::Transformer::new(source, &options.module_name, &parsed.program);
    let Some((code, diagnostics)) = transformer.program(&parsed.program) else { return Ok(None) };
    let map = options.source_map.then(|| code.source_map(filename, source));
    let warnings = diagnostics
        .into_iter()
        .map(|(offset, message)| {
            let (line, column) = position(source, offset as usize);
            Warning { message, line, column }
        })
        .collect();
    Ok(Some(Output { code: code.s, map, warnings }))
}

fn position(source: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.bytes().filter(|&b| b == b'\n').count() as u32 + 1;
    (line, before[line_start..].encode_utf16().count() as u32)
}
