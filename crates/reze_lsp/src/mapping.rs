use std::collections::BTreeMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeDescription, Diagnostic as LspDiagnostic,
    DiagnosticRelatedInformation, DiagnosticSeverity as LspSeverity, Hover, HoverContents,
    InlayHint, InlayHintKind, InlayHintLabel, Location, MarkupContent, MarkupKind, NumberOrString,
    Position as LspPosition, Range as LspRange, TextEdit, Uri, WorkspaceEdit,
};
use reze_compiler::diagnostic::LineIndex;
use reze_compiler::{Diagnostic, Position as CompilerPosition, Severity};

pub fn compiler_position_to_lsp(position: &CompilerPosition) -> LspPosition {
    LspPosition { line: position.line.saturating_sub(1), character: position.column }
}

pub fn compiler_range_to_lsp(diagnostic: &Diagnostic) -> LspRange {
    LspRange {
        start: compiler_position_to_lsp(&diagnostic.start),
        end: compiler_position_to_lsp(&diagnostic.end),
    }
}

pub fn severity_to_lsp(severity: Severity) -> LspSeverity {
    match severity {
        Severity::Error => LspSeverity::ERROR,
        Severity::Warn => LspSeverity::WARNING,
        Severity::Info => LspSeverity::INFORMATION,
    }
}

pub fn offset_to_lsp_position(text: &str, index: &LineIndex, offset: u32) -> LspPosition {
    let (line, column) = index.locate(text, offset as usize);
    LspPosition { line, character: column }
}

pub fn lsp_position_to_offset(text: &str, position: LspPosition) -> u32 {
    let mut line_start = 0usize;
    let mut line_index = 0u32;
    for (offset, byte) in text.bytes().enumerate() {
        if line_index == position.line {
            break;
        }
        if byte == b'\n' {
            line_index += 1;
            line_start = offset + 1;
        }
    }
    if line_index != position.line {
        return text.len() as u32;
    }
    let line_end = text[line_start..].find('\n').map_or(text.len(), |i| line_start + i);
    let line = &text[line_start..line_end];
    let mut units = 0u32;
    for (byte_index, ch) in line.char_indices() {
        if units >= position.character {
            return (line_start + byte_index) as u32;
        }
        units += ch.len_utf16() as u32;
    }
    line_end as u32
}

pub fn diagnostic_to_lsp(
    uri: &str,
    diagnostic: &Diagnostic,
    source: &str,
    others: &BTreeMap<String, String>,
) -> LspDiagnostic {
    let mut related_information = Vec::new();
    if let Ok(self_uri) = uri.parse::<Uri>() {
        for label in &diagnostic.labels {
            related_information.push(DiagnosticRelatedInformation {
                location: Location {
                    uri: self_uri.clone(),
                    range: offsets_to_lsp_range(source, label.start, label.end),
                },
                message: label.message.clone(),
            });
        }
    }
    for related in &diagnostic.related {
        let Ok(related_uri) = related.file.parse::<Uri>() else { continue };
        let range = match others.get(&related.file) {
            Some(text) => offsets_to_lsp_range(text, related.start, related.end),
            None if related.file == uri => offsets_to_lsp_range(source, related.start, related.end),
            None => LspRange::default(),
        };
        related_information.push(DiagnosticRelatedInformation {
            location: Location { uri: related_uri, range },
            message: related.message.clone(),
        });
    }
    LspDiagnostic {
        range: compiler_range_to_lsp(diagnostic),
        severity: Some(severity_to_lsp(diagnostic.severity)),
        code: Some(NumberOrString::String(diagnostic.code.name().to_string())),
        code_description: Some(CodeDescription {
            href: diagnostic.docs().parse().unwrap_or_else(|_| fallback_docs_uri()),
        }),
        source: Some("reze".to_string()),
        message: diagnostic.message.clone(),
        related_information: Some(related_information).filter(|infos| !infos.is_empty()),
        tags: None,
        data: Some(diagnostic_data(diagnostic)),
    }
}

fn fallback_docs_uri() -> Uri {
    "https://github.com/knst0/reze-js/blob/main/packages/compiler/skills/compiler-diagnostics/SKILL.md"
        .parse()
        .expect("static docs url parses")
}

pub fn offsets_to_lsp_range(text: &str, start: u32, end: u32) -> LspRange {
    let index = LineIndex::new(text);
    LspRange {
        start: offset_to_lsp_position(text, &index, start),
        end: offset_to_lsp_position(text, &index, end.max(start)),
    }
}

pub fn diagnostic_data(diagnostic: &Diagnostic) -> serde_json::Value {
    let data: BTreeMap<&str, &str> =
        diagnostic.data.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    serde_json::json!({
        "file": diagnostic.file,
        "path": diagnostic.path,
        "labels": diagnostic.labels.iter().map(|label| serde_json::json!({
            "start": label.start, "end": label.end, "message": label.message,
        })).collect::<Vec<_>>(),
        "related": diagnostic.related.iter().map(|related| serde_json::json!({
            "file": related.file, "start": related.start, "end": related.end,
            "message": related.message,
        })).collect::<Vec<_>>(),
        "fixes": diagnostic.fixes.iter().map(|fix| serde_json::json!({
            "title": fix.title,
            "edits": fix.edits.iter().map(|edit| serde_json::json!({
                "start": edit.start, "end": edit.end, "text": edit.text,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "data": data,
        "docs": diagnostic.docs(),
        "rendered": diagnostic.rendered,
    })
}

pub fn inlay_hints(diagnostics: &[Diagnostic]) -> Vec<InlayHint> {
    diagnostics.iter().filter_map(inlay_hint).collect()
}

fn inlay_hint(diagnostic: &Diagnostic) -> Option<InlayHint> {
    let (label, kind) = inlay_label(diagnostic)?;
    Some(InlayHint {
        position: compiler_position_to_lsp(&diagnostic.end),
        label: InlayHintLabel::String(label),
        kind: Some(kind),
        text_edits: None,
        tooltip: Some(lsp_types::InlayHintTooltip::String(diagnostic.message.clone())),
        padding_left: Some(true),
        padding_right: None,
        data: None,
    })
}

fn inlay_label(diagnostic: &Diagnostic) -> Option<(String, InlayHintKind)> {
    let datum = |key: &str| {
        diagnostic.data.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
    };
    match diagnostic.code.name() {
        "SIGNAL_FOLDED" => {
            Some((format!("folded{}", scope_suffix(datum("scope"))), InlayHintKind::PARAMETER))
        }
        "DEAD_BRANCH_REMOVED" => {
            Some(("dead branch removed".to_string(), InlayHintKind::PARAMETER))
        }
        "COMPUTED_INLINED" => {
            Some((format!("inlined{}", scope_suffix(datum("scope"))), InlayHintKind::PARAMETER))
        }
        "PROPS_REWRITTEN" => Some(("props: reactive reads".to_string(), InlayHintKind::PARAMETER)),
        "STORE_UNPROXIED" => Some((
            format!("store: signals{}", scope_suffix(datum("scope"))),
            InlayHintKind::PARAMETER,
        )),
        "STATIC_COMPONENT" => Some(("static".to_string(), InlayHintKind::TYPE)),
        "CLIENT_COMPONENT" => {
            let reason = datum("reason");
            Some((format!("client{}", short_reason(reason)), InlayHintKind::TYPE))
        }
        "ISLAND" => {
            let id = datum("id");
            let features = datum("features");
            let mode = datum("mode");
            Some((island_label(id, features, mode), InlayHintKind::TYPE))
        }
        "LAZY_ISLAND" => Some((format!("lazy: {}", datum("mode")), InlayHintKind::TYPE)),
        _ => None,
    }
}

fn scope_suffix(scope: &str) -> String {
    if scope.is_empty() { String::new() } else { format!(" ({scope})") }
}

fn short_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut clipped: String = trimmed.chars().take(64).collect();
    if trimmed.chars().count() > 64 {
        clipped.push('…');
    }
    format!(": {clipped}")
}

fn island_label(id: &str, features: &str, mode: &str) -> String {
    let mut label = format!("island {id}");
    if !features.is_empty() {
        label.push_str(&format!(" [{features}]"));
    }
    if !mode.is_empty() && mode != "eager" {
        label.push_str(&format!(" ({mode})"));
    }
    label
}

pub fn explain_at(diagnostics: &[Diagnostic], offset: u32) -> Option<String> {
    let mut hits: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.start.offset <= offset && offset <= diagnostic.end.offset)
        .collect();
    if hits.is_empty() {
        return None;
    }
    hits.sort_by_key(|diagnostic| {
        (explain_priority(diagnostic), diagnostic.end.offset - diagnostic.start.offset)
    });
    Some(explain(hits[0]))
}

fn explain_priority(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.code.name() {
        "ISLAND" | "LAZY_ISLAND" => 0,
        "CLIENT_COMPONENT" | "STATIC_COMPONENT" => 1,
        _ => 2,
    }
}

pub fn explain(diagnostic: &Diagnostic) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} {}\n\n", diagnostic.code.name(), diagnostic.message));
    if !diagnostic.path.is_empty() {
        out.push_str(&format!("in {}\n\n", diagnostic.path.join(" › ")));
    }
    match diagnostic.code.name() {
        "ISLAND" | "LAZY_ISLAND" => explain_island(diagnostic, &mut out),
        "CLIENT_COMPONENT" => explain_client(diagnostic, &mut out),
        _ => explain_generic(diagnostic, &mut out),
    }
    out.push_str(&format!("\n[repair guide]({})\n", diagnostic.docs()));
    out.push_str(&format!("\n```text\n{}\n```\n", diagnostic.rendered));
    out
}

fn explain_island(diagnostic: &Diagnostic, out: &mut String) {
    let datum = |key: &str| {
        diagnostic.data.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
    };
    let id = datum("id");
    let features = datum("features");
    let mode = datum("mode");
    if !id.is_empty() {
        out.push_str(&format!("island id: `{id}`\n\n"));
    }
    if !mode.is_empty() {
        out.push_str(&format!("load mode: `{mode}`\n\n"));
    }
    if features.is_empty() {
        out.push_str("runtime features: none\n\n");
    } else {
        out.push_str(&format!("runtime features: `{features}`\n\n"));
    }
    out.push_str(
        "why this island: a static component renders a client component here with serializable props, so the server renders the HTML and the browser hydrates only this subtree. Hover the inner component for why it runs on the client.\n\n",
    );
    explain_generic(diagnostic, out);
}

fn explain_client(diagnostic: &Diagnostic, out: &mut String) {
    out.push_str("why on the client:\n\n");
    if diagnostic.related.is_empty() {
        out.push_str(&format!("1. {}\n\n", first_reason(diagnostic)));
    } else {
        out.push_str(&format!("1. {}\n", first_reason(diagnostic)));
        for (index, related) in diagnostic.related.iter().enumerate() {
            out.push_str(&format!(
                "{}. {} — {}:{}:{}\n",
                index + 2,
                related.message,
                related.file,
                related.start,
                related.end
            ));
        }
        out.push('\n');
    }
    explain_generic(diagnostic, out);
}

fn first_reason(diagnostic: &Diagnostic) -> String {
    if diagnostic.related.is_empty() {
        for label in &diagnostic.labels {
            if !label.message.is_empty() {
                return label.message.clone();
            }
        }
    }
    diagnostic.message.clone()
}

fn explain_generic(diagnostic: &Diagnostic, out: &mut String) {
    if !diagnostic.data.is_empty() {
        out.push_str("| fact | value |\n| --- | --- |\n");
        for (key, value) in &diagnostic.data {
            out.push_str(&format!("| `{key}` | `{value}` |\n"));
        }
        out.push('\n');
    }
    for fix in &diagnostic.fixes {
        out.push_str(&format!("fix: {}\n\n", fix.title));
    }
}

pub fn hover_at(source: &str, diagnostics: &[Diagnostic], position: LspPosition) -> Option<Hover> {
    let offset = lsp_position_to_offset(source, position);
    let mut hits: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.start.offset <= offset && offset <= diagnostic.end.offset)
        .collect();
    if hits.is_empty() {
        return None;
    }
    hits.sort_by_key(|diagnostic| {
        (explain_priority(diagnostic), diagnostic.end.offset - diagnostic.start.offset)
    });
    let top = hits[0];
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: explain(top),
        }),
        range: Some(compiler_range_to_lsp(top)),
    })
}

#[allow(clippy::mutable_key_type)]
pub fn code_actions(
    uri: &str,
    source: &str,
    diagnostics: &[Diagnostic],
    range: LspRange,
) -> Vec<CodeAction> {
    let Ok(action_uri) = uri.parse::<Uri>() else { return Vec::new() };
    let mut actions = Vec::new();
    for diagnostic in diagnostics {
        if !ranges_overlap(&compiler_range_to_lsp(diagnostic), &range) {
            continue;
        }
        let lsp_diagnostic = diagnostic_to_lsp(uri, diagnostic, source, &BTreeMap::new());
        for fix in &diagnostic.fixes {
            let mut edits = Vec::new();
            for edit in &fix.edits {
                edits.push(TextEdit {
                    range: offsets_to_lsp_range(source, edit.start, edit.end),
                    new_text: edit.text.clone(),
                });
            }
            let mut changes = std::collections::HashMap::new();
            changes.insert(action_uri.clone(), edits);
            actions.push(CodeAction {
                title: format!("{} ({})", fix.title, diagnostic.code.name()),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![lsp_diagnostic.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
            });
        }
    }
    actions
}

fn ranges_overlap(first: &LspRange, second: &LspRange) -> bool {
    if first.end.line < second.start.line
        || second.end.line < first.start.line
        || (first.end.line == second.start.line && first.end.character < second.start.character)
        || (second.end.line == first.start.line && second.end.character < first.start.character)
    {
        return false;
    }
    true
}
