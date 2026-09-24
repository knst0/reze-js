pub mod catalog;

use oxc_span::Span;

pub use catalog::Code;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// `line` is 1-based; `column` is 0-based in UTF-16 code units; `offset` is a byte offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub offset: u32,
    pub line: u32,
    pub column: u32,
}

/// A secondary span; `start`/`end` are byte offsets.
#[derive(Clone, Debug)]
pub struct Label {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// Replaces bytes `start..end` of the source with `text`.
#[derive(Clone, Debug)]
pub struct Edit {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// Edits that, applied together, remove the diagnostic.
#[derive(Clone, Debug)]
pub struct Fix {
    pub title: String,
    pub edits: Vec<Edit>,
}

#[derive(Clone, Debug)]
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
    pub data: Vec<(String, String)>,
    /// Message, `in` line, location, code frame and fixes, without the once-per-code footer.
    pub rendered: String,
}

impl Diagnostic {
    pub fn docs(&self) -> String {
        docs_url(self.code)
    }
}

pub fn docs_url(code: Code) -> String {
    format!("{}#{}", catalog::DOCS_BASE, code.name().to_ascii_lowercase())
}

/// Footer printed after the first diagnostic of each code.
pub fn footer(code: Code) -> String {
    format!(
        "  repair guide: {}#{}\n                {}",
        catalog::SKILL_PATH,
        code.name().to_ascii_lowercase(),
        docs_url(code)
    )
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.rendered)
    }
}

/// A diagnostic before positions are resolved and it is rendered.
pub struct Report {
    pub code: Code,
    pub span: Span,
    pub message: String,
    pub path: Vec<String>,
    pub labels: Vec<Label>,
    pub fixes: Vec<Fix>,
    pub data: Vec<(String, String)>,
}

impl Report {
    /// `message` is the text after the `[CODE]` prefix.
    pub fn new(code: Code, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
            path: Vec::new(),
            labels: Vec::new(),
            fixes: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn fix(mut self, title: impl Into<String>, edits: Vec<Edit>) -> Self {
        self.fixes.push(Fix { title: title.into(), edits });
        self
    }

    pub fn label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label { start: span.start, end: span.end, message: message.into() });
        self
    }

    pub fn data(mut self, key: &str, value: impl Into<String>) -> Self {
        self.data.push((key.to_string(), value.into()));
        self
    }
}

pub fn resolve(mut reports: Vec<Report>, source: &str, file: &str) -> Vec<Diagnostic> {
    reports.sort_by_key(|report| report.span.start);
    let lines = LineIndex::new(source);
    reports.into_iter().map(|report| resolve_one(report, source, file, &lines)).collect()
}

fn resolve_one(report: Report, source: &str, file: &str, lines: &LineIndex) -> Diagnostic {
    let severity = report.code.severity();
    let message = format!("[{}] {}", report.code.name(), report.message);
    let start = lines.position(source, report.span.start);
    let end = lines.position(source, report.span.end.max(report.span.start));
    let rendered = render(&message, &report, file, source, lines, start);
    Diagnostic {
        code: report.code,
        severity,
        message,
        file: file.to_string(),
        start,
        end,
        path: report.path,
        labels: report.labels,
        fixes: report.fixes,
        data: report.data,
        rendered,
    }
}

fn render(
    message: &str,
    report: &Report,
    file: &str,
    source: &str,
    lines: &LineIndex,
    start: Position,
) -> String {
    let mut out = String::from(message);
    if !report.path.is_empty() {
        out.push_str("\n  in ");
        out.push_str(&report.path.join(" › "));
    }
    out.push_str(&format!("\n  at {file}:{}:{}", start.line, start.column + 1));
    code_frame(&mut out, source, lines, report.span, start.line);
    for label in &report.labels {
        let at = lines.position(source, label.start);
        out.push_str(&format!(
            "\n  note: {} ({file}:{}:{})",
            label.message,
            at.line,
            at.column + 1
        ));
    }
    for fix in &report.fixes {
        out.push_str("\n  fix: ");
        out.push_str(&fix.title);
    }
    out
}

fn code_frame(out: &mut String, source: &str, lines: &LineIndex, span: Span, line: u32) {
    let first = line.saturating_sub(1).max(1);
    let last = (line + 1).min(lines.count());
    let width = last.to_string().len();
    for number in first..=last {
        let text = lines.text(source, number);
        let gutter = if number == line { '>' } else { ' ' };
        out.push_str(&format!("\n{gutter} {number:>width$} | {text}"));
        if number == line {
            let line_start = lines.start(number);
            let from = span.start as usize - line_start;
            let to = (span.end as usize).min(line_start + text.len()).max(span.start as usize + 1)
                - line_start;
            let pad: String =
                text[..from].chars().map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
            let carets = text.get(from..to).map_or(1, |s| s.chars().count().max(1));
            out.push_str(&format!("\n  {:width$} | {pad}{}", "", "^".repeat(carets)));
        }
    }
}

/// Line starts of a text, for random-access `offset → (line, column)`.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    /// 0-based line and UTF-16 column.
    pub fn locate(&self, text: &str, offset: usize) -> (u32, u32) {
        let offset = offset.min(text.len());
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        (line as u32, utf16_len(&text[self.starts[line]..offset]))
    }

    fn position(&self, text: &str, offset: u32) -> Position {
        let (line, column) = self.locate(text, offset as usize);
        Position { offset, line: line + 1, column }
    }

    fn count(&self) -> u32 {
        self.starts.len() as u32
    }

    /// Start offset of 1-based `line`.
    fn start(&self, line: u32) -> usize {
        self.starts[line as usize - 1]
    }

    /// Text of 1-based `line` without its line break.
    fn text<'t>(&self, text: &'t str, line: u32) -> &'t str {
        let start = self.start(line);
        let end = self.starts.get(line as usize).map_or(text.len(), |&next| next - 1);
        text[start..end].strip_suffix('\r').unwrap_or(&text[start..end])
    }
}

pub fn utf16_len(s: &str) -> u32 {
    if s.is_ascii() { s.len() as u32 } else { s.encode_utf16().count() as u32 }
}
