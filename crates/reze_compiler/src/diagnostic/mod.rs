mod catalog;
mod render;
mod skill;

use std::collections::BTreeMap;

use oxc_span::Span;
use serde::Serialize;

pub use catalog::{CATALOG, Code, Entry, Example};
pub use render::LineIndex;
pub use skill::render_skill;

pub const DOCS_BASE: &str = "https://github.com/knst0/reze-js/blob/main/packages/compiler/skills/reze-compiler-diagnostics/SKILL.md";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Info => "info",
        }
    }
}

/// `offset` is a byte offset, `line` 1-based, `column` 0-based in UTF-16 code units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Position {
    pub offset: u32,
    pub line: u32,
    pub column: u32,
}

/// A secondary span in byte offsets.
#[derive(Clone, Debug, Serialize)]
pub struct Label {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// Replaces source bytes `start..end` with `text`.
#[derive(Clone, Debug, Serialize)]
pub struct Edit {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// Edits that, applied together, remove the diagnostic.
#[derive(Clone, Debug, Serialize)]
pub struct Fix {
    pub title: String,
    pub edits: Vec<Edit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    pub file: String,
    pub start: Position,
    pub end: Position,
    /// Enclosing components (`<Name>`) and elements, root first.
    pub path: Vec<String>,
    pub labels: Vec<Label>,
    pub fixes: Vec<Fix>,
    pub data: BTreeMap<String, String>,
    pub docs: String,
    /// Message, `in` path, location, code frame and fixes.
    pub rendered: String,
}

/// A diagnostic before its position is resolved and its text rendered from the catalog.
pub struct Report {
    pub code: Code,
    pub span: Span,
    pub path: Vec<String>,
    args: Vec<(&'static str, String)>,
    labels: Vec<Label>,
    fixes: Vec<Vec<Edit>>,
}

impl Report {
    pub fn new(code: Code, span: Span) -> Self {
        Self {
            code,
            span,
            path: Vec::new(),
            args: Vec::new(),
            labels: Vec::new(),
            fixes: Vec::new(),
        }
    }

    /// Fills `{key}` in the catalog message and fix title, and becomes `data.key`.
    pub fn arg(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.args.push((key, value.into()));
        self
    }

    pub fn label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label { start: span.start, end: span.end, message: message.into() });
        self
    }

    /// Edits of the fix whose title is the catalog's `fix` template.
    pub fn fix(mut self, edits: Vec<Edit>) -> Self {
        self.fixes.push(edits);
        self
    }
}

pub fn docs_url(code: Code) -> String {
    format!("{DOCS_BASE}#{}", code.name().to_ascii_lowercase())
}

/// Sorts `reports` by start and resolves each against `source`.
pub fn resolve(mut reports: Vec<Report>, source: &str, file: &str) -> Vec<Diagnostic> {
    reports.sort_by_key(|report| report.span.start);
    let lines = LineIndex::new(source);
    reports.into_iter().map(|report| resolve_one(report, source, file, &lines)).collect()
}

fn resolve_one(report: Report, source: &str, file: &str, lines: &LineIndex) -> Diagnostic {
    let entry = report.code.entry();
    #[cfg(debug_assertions)]
    render::assert_args(entry, &report.args);
    let message = format!("[{}] {}", entry.name, render::fill(entry.message, &report.args));
    let fixes: Vec<Fix> = report
        .fixes
        .into_iter()
        .map(|edits| Fix {
            title: render::fill(entry.fix.unwrap_or_default(), &report.args),
            edits,
        })
        .collect();
    let start = lines.position(source, report.span.start);
    let end = lines.position(source, report.span.end.max(report.span.start));
    let rendered = render::text(
        &message,
        &report.path,
        &report.labels,
        &fixes,
        file,
        source,
        lines,
        report.span,
    );
    Diagnostic {
        code: report.code,
        severity: entry.severity,
        message,
        file: file.to_string(),
        start,
        end,
        path: report.path,
        labels: report.labels,
        fixes,
        data: report.args.into_iter().map(|(key, value)| (key.to_string(), value)).collect(),
        docs: docs_url(report.code),
        rendered,
    }
}

pub fn utf16_len(s: &str) -> u32 {
    if s.is_ascii() { s.len() as u32 } else { s.encode_utf16().count() as u32 }
}
