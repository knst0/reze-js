use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeDescription, Diagnostic,
    DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString, Position, Range,
    TextEdit, Uri, WorkspaceEdit,
};
use reze_compiler::{Diagnostic as Report, LineIndex, Position as CompilerPosition, Severity};

pub struct Document {
    pub text: String,
    pub version: i32,
    pub index: LineIndex,
    pub reports: Vec<Report>,
}

impl Document {
    pub fn offset_at(&self, at: Position) -> Option<u32> {
        offset_at(&self.text, at).map(|offset| offset as u32)
    }

    pub fn span_range(&self, start: u32, end: u32) -> Range {
        offset_range(&self.text, &self.index, start, end)
    }
}

pub fn offset_at(text: &str, at: Position) -> Option<usize> {
    let mut line_start = 0usize;
    for _ in 0..at.line {
        line_start = text[line_start..].find('\n').map(|found| line_start + found + 1)?;
    }
    let line_end = text[line_start..].find('\n').map_or(text.len(), |found| line_start + found);
    let mut taken = 0usize;
    let mut units = 0u32;
    for c in text[line_start..line_end].chars() {
        if units >= at.character {
            break;
        }
        units += c.len_utf16() as u32;
        taken += c.len_utf8();
        if units > at.character {
            taken -= c.len_utf8();
            break;
        }
    }
    Some(line_start + taken)
}

pub fn position(at: CompilerPosition) -> Position {
    Position { line: at.line.saturating_sub(1), character: at.column }
}

fn offset_range(text: &str, index: &LineIndex, start: u32, end: u32) -> Range {
    let (start_line, start_char) = index.locate(text, floored(text, start));
    let (end_line, end_char) = index.locate(text, floored(text, end.max(start)));
    Range {
        start: Position { line: start_line, character: start_char },
        end: Position { line: end_line, character: end_char },
    }
}

fn floored(text: &str, offset: u32) -> usize {
    let mut at = (offset as usize).min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

pub fn diagnostic(uri: &Uri, text: &str, index: &LineIndex, report: &Report) -> Diagnostic {
    Diagnostic {
        range: Range { start: position(report.start), end: position(report.end) },
        severity: Some(match report.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warn => DiagnosticSeverity::WARNING,
            Severity::Info => DiagnosticSeverity::INFORMATION,
        }),
        code: Some(NumberOrString::String(report.code.name().to_string())),
        code_description: report.docs.parse::<Uri>().ok().map(|href| CodeDescription { href }),
        source: Some(String::from("reze")),
        message: report.message.clone(),
        related_information: (!report.labels.is_empty()).then(|| {
            report
                .labels
                .iter()
                .map(|label| DiagnosticRelatedInformation {
                    location: Location {
                        uri: uri.clone(),
                        range: offset_range(text, index, label.start, label.end),
                    },
                    message: label.message.clone(),
                })
                .collect()
        }),
        data: Some(serde_json::json!({"path": report.path, "data": report.data})),
        ..Default::default()
    }
}

pub fn code_actions(
    uri: &Uri,
    text: &str,
    index: &LineIndex,
    reports: &[Report],
    range: Range,
    only: Option<Vec<CodeActionKind>>,
) -> Vec<CodeActionOrCommand> {
    if only.is_some_and(|kinds| !kinds.is_empty() && !kinds.contains(&CodeActionKind::QUICKFIX)) {
        return Vec::new();
    }
    let mut actions = Vec::new();
    for report in reports {
        let published = diagnostic(uri, text, index, report);
        if !overlaps(published.range, range) {
            continue;
        }
        for (n, fix) in report.fixes.iter().enumerate() {
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: fix.title.clone(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![published.clone()]),
                is_preferred: (n == 0).then_some(true),
                edit: Some(WorkspaceEdit {
                    changes: Some(HashMap::from([(
                        uri.clone(),
                        fix.edits
                            .iter()
                            .map(|edit| TextEdit {
                                range: offset_range(text, index, edit.start, edit.end),
                                new_text: edit.text.clone(),
                            })
                            .collect(),
                    )])),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
    }
    actions
}

fn overlaps(a: Range, b: Range) -> bool {
    (a.start.line, a.start.character) <= (b.end.line, b.end.character)
        && (b.start.line, b.start.character) <= (a.end.line, a.end.character)
}

/// File path of a document URI, for dialect detection and diagnostic labels.
pub fn filename(uri: &Uri) -> String {
    let text = uri.as_str();
    let path = text
        .strip_prefix("file://")
        .map_or(text, |rest| rest.find('/').map_or(rest, |slash| &rest[slash..]));
    percent_decode(path)
}

fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| path.to_string())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(text: &str) -> Uri {
        text.parse().unwrap()
    }

    #[test]
    fn filename_strips_file_uri() {
        assert_eq!(filename(&uri("file:///a/b.tsx")), "/a/b.tsx");
        assert_eq!(filename(&uri("file://localhost/a/b.ts")), "/a/b.ts");
        assert_eq!(filename(&uri("file:///a%20b/c%2B.tsx")), "/a b/c+.tsx");
        assert_eq!(filename(&uri("file:///C%3A/a.tsx")), "/C:/a.tsx");
    }

    #[test]
    fn filename_keeps_non_file_uri() {
        assert_eq!(filename(&uri("untitled:Untitled-1")), "untitled:Untitled-1");
    }

    #[test]
    fn position_is_zero_based() {
        let at = position(CompilerPosition { offset: 7, line: 3, column: 5 });
        assert_eq!(at, Position { line: 2, character: 5 });
    }

    #[test]
    fn offset_at_round_trips_through_locate() {
        let text = "let a = 1;\nlet b = \"\u{1F389}xy\";\nlet c = a;\n";
        let index = LineIndex::new(text);
        for offset in [0, 5, 11, 20, 24, 26, 27, 31, 36] {
            let (line, character) = index.locate(text, offset);
            assert_eq!(offset_at(text, Position { line, character }), Some(offset), "{offset}");
        }
    }

    #[test]
    fn offset_at_handles_emoji_and_line_bounds() {
        let text = "a\u{1F389}b\ncd";
        assert_eq!(offset_at(text, Position { line: 0, character: 0 }), Some(0));
        assert_eq!(offset_at(text, Position { line: 0, character: 1 }), Some(1));
        assert_eq!(offset_at(text, Position { line: 0, character: 3 }), Some(5));
        assert_eq!(offset_at(text, Position { line: 0, character: 4 }), Some(6));
        assert_eq!(offset_at(text, Position { line: 0, character: 99 }), Some(6));
        assert_eq!(offset_at(text, Position { line: 1, character: 1 }), Some(8));
        assert_eq!(offset_at(text, Position { line: 2, character: 0 }), None);
        assert_eq!(offset_at(text, Position { line: 0, character: 2 }), Some(1));
    }
}
