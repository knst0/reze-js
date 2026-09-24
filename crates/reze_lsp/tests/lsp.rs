use std::collections::BTreeMap;

use lsp_types::{CodeActionKind, Position as LspPosition, Range as LspRange};
use reze_lsp::analysis::{Config, analyze_program, compile_single, resolve_specifier};
use reze_lsp::mapping::{
    code_actions, compiler_range_to_lsp, diagnostic_data, diagnostic_to_lsp, explain_at, hover_at,
    inlay_hints, lsp_position_to_offset, offsets_to_lsp_range,
};
use reze_lsp::server::{path_to_uri, scan_workspace};

fn config() -> Config {
    Config::default()
}

fn find_by_code(
    diagnostics: &[reze_compiler::Diagnostic],
    code: &str,
) -> Option<reze_compiler::Diagnostic> {
    diagnostics.iter().find(|d| d.code.name() == code).cloned()
}

fn datum(diagnostic: &reze_compiler::Diagnostic, key: &str) -> String {
    diagnostic.data.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default()
}

fn apply_edits(source: &str, edits: &[(u32, u32, String)]) -> String {
    let mut out = source.to_string();
    let mut ordered = edits.to_vec();
    ordered.sort_by_key(|(start, _, _)| *start);
    let mut delta: i64 = 0;
    for (start, end, text) in ordered {
        let from = (start as i64 + delta) as usize;
        let to = (end as i64 + delta) as usize;
        out.replace_range(from..to, &text);
        delta += text.len() as i64 - (end - start) as i64;
    }
    out
}

#[test]
fn warn_diagnostic_maps_to_lsp_range_and_fix() {
    let uri = "file:///app.tsx";
    let source = "const el = <div classList=\"x\" />;";
    let diagnostics = compile_single(source, uri, &config());
    let diagnostic = find_by_code(&diagnostics, "UNKNOWN_ATTRIBUTE").expect("classList warns");
    assert_eq!(diagnostic.severity, reze_compiler::Severity::Warn);
    let lsp = diagnostic_to_lsp(uri, &diagnostic, source, &BTreeMap::new());
    assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::WARNING));
    assert_eq!(lsp.code, Some(lsp_types::NumberOrString::String("UNKNOWN_ATTRIBUTE".to_string())));
    assert_eq!(lsp.source.as_deref(), Some("reze"));
    assert_eq!((lsp.range.start.line, lsp.range.start.character), (0, 16));
    assert!(lsp.code_description.unwrap().href.as_str().ends_with("#unknown_attribute"));
    let range = LspRange {
        start: LspPosition { line: 0, character: 0 },
        end: LspPosition { line: 0, character: 100 },
    };
    let actions = code_actions(uri, source, &diagnostics, range);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, Some(CodeActionKind::QUICKFIX));
    let title = actions[0].title.clone();
    assert!(title.contains("class") && title.contains("UNKNOWN_ATTRIBUTE"));
    let edits: Vec<(u32, u32, String)> =
        diagnostic.fixes[0].edits.iter().map(|e| (e.start, e.end, e.text.clone())).collect();
    let fixed = apply_edits(source, &edits);
    assert_eq!(fixed, "const el = <div class=\"x\" />;");
    let after = compile_single(&fixed, uri, &config());
    assert!(find_by_code(&after, "UNKNOWN_ATTRIBUTE").is_none());
}

#[test]
fn folded_signal_becomes_a_parameter_hint() {
    let uri = "file:///fold.tsx";
    let source = "import { signal } from \"reze-js\";\nconst [title] = signal(\"Reze\");\nconst el = <h1>{title()}</h1>;";
    let diagnostics = compile_single(source, uri, &config());
    let diagnostic = find_by_code(&diagnostics, "SIGNAL_FOLDED").expect("signal folds");
    assert_eq!(datum(&diagnostic, "scope"), "module");
    let hints = inlay_hints(&diagnostics);
    let hint = hints.iter().find(|h| h.position == compiler_range_to_lsp(&diagnostic).end);
    assert_eq!(
        hint.map(|h| format!("{:?}", h.label)).unwrap_or_default(),
        format!("{:?}", lsp_types::InlayHintLabel::String("folded (module)".to_string()))
    );
}

#[test]
fn program_links_islands_with_reason_chains() {
    let counter = "file:///counter.tsx";
    let page = "file:///page.tsx";
    let mut files = BTreeMap::new();
    files.insert(
        counter.to_string(),
        "import { signal } from \"reze-js\";\nexport function Counter(props) {\n  const [count, setCount] = signal(props.start);\n  return <button onClick={() => setCount(count() + 1)}>{count()}</button>;\n}".to_string(),
    );
    files.insert(
        page.to_string(),
        "import { renderToString } from \"reze-js\";\nimport { Counter } from \"./counter\";\nexport function Page() {\n  return <main><Counter start={1} /></main>;\n}\nexport const html = renderToString(() => <Page />);".to_string(),
    );
    let reports = analyze_program(&files, &config());
    let page_diags = reports.get(page).cloned().unwrap_or_default();
    let counter_diags = reports.get(counter).cloned().unwrap_or_default();
    let island = find_by_code(&page_diags, "ISLAND").expect("boundary reports island");
    let island_id = datum(&island, "id");
    assert!(
        island_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() && (c.is_ascii_lowercase() || c.is_ascii_digit()))
    );
    let features = datum(&island, "features");
    let expected_label = if features.is_empty() {
        format!("island {island_id}")
    } else {
        format!("island {island_id} [{features}]")
    };
    let hints = inlay_hints(&page_diags);
    assert!(hints.iter().any(|h| format!("{:?}", h.label)
        == format!("{:?}", lsp_types::InlayHintLabel::String(expected_label.clone()))));
    let offset = island.start.offset + 1;
    let markdown = explain_at(&page_diags, offset).expect("island explains itself");
    assert!(markdown.contains(&format!("`{island_id}`")));
    assert!(markdown.contains("why this island"));
    let client = find_by_code(&counter_diags, "CLIENT_COMPONENT").expect("counter runs on client");
    assert!(datum(&client, "component") == "Counter");
    let others: BTreeMap<String, String> = files.clone();
    let lsp = diagnostic_to_lsp(counter, &client, &files[counter], &others);
    let related = lsp.related_information.unwrap_or_default();
    assert!(related.iter().all(|r| r.location.range.start <= r.location.range.end));
    let page_single = compile_single(&files[page], page, &config());
    assert!(find_by_code(&page_single, "ISLAND").is_none());
    assert!(find_by_code(&page_single, "CLIENT_COMPONENT").is_none());
    assert!(find_by_code(&page_single, "STATIC_COMPONENT").is_none());
    assert!(find_by_code(&page_diags, "STATIC_COMPONENT").is_some());
}

#[test]
fn client_reason_chain_resolves_in_open_documents() {
    let counter = "file:///chain/counter.tsx";
    let page = "file:///chain/page.tsx";
    let mut files = BTreeMap::new();
    files.insert(
        counter.to_string(),
        "import { signal } from \"reze-js\";\nexport function Counter(props) {\n  const [count, setCount] = signal(0);\n  return <button onClick={() => setCount(count() + 1)}>{count()}</button>;\n}".to_string(),
    );
    files.insert(
        page.to_string(),
        "import { renderToString } from \"reze-js\";\nimport { Counter } from \"./counter\";\nexport function Page() {\n  return <main><Counter start={1} ref={undefined} /></main>;\n}\nexport const html = renderToString(() => <Page />);".to_string(),
    );
    let reports = analyze_program(&files, &config());
    let page_diags = reports.get(page).cloned().unwrap_or_default();
    let client = find_by_code(&page_diags, "CLIENT_COMPONENT").expect("page runs on client");
    assert!(!client.related.is_empty());
    assert_eq!(client.related[0].file, counter);
    let others: BTreeMap<String, String> = files.clone();
    let lsp = diagnostic_to_lsp(page, &client, &files[page], &others);
    let related = lsp.related_information.unwrap_or_default();
    let counter_text = &files[counter];
    let expected =
        offsets_to_lsp_range(counter_text, client.related[0].start, client.related[0].end);
    assert!(
        related.iter().any(|r| r.location.uri.as_str() == counter && r.location.range == expected)
    );
    let markdown = explain_at(&page_diags, client.start.offset).expect("client explains itself");
    assert!(markdown.contains("why on the client"));
    assert!(markdown.contains(counter));
}

#[test]
fn hover_prefers_island_over_component() {
    let uri = "file:///hover.tsx";
    let source = "const el = <div clas=\"box\" />;";
    let diagnostics = compile_single(source, uri, &config());
    let diagnostic = find_by_code(&diagnostics, "UNKNOWN_ATTRIBUTE").expect("typo warns");
    let position = LspPosition { line: diagnostic.end.line - 1, character: diagnostic.end.column };
    let hover = hover_at(source, &diagnostics, position).expect("typo hovers");
    let lsp_types::HoverContents::Markup(markup) = hover.contents else {
        panic!("hover is markup")
    };
    assert!(markup.value.contains("UNKNOWN_ATTRIBUTE"));
    assert_eq!(hover.range, Some(compiler_range_to_lsp(&diagnostic)));
}

#[test]
fn utf16_positions_round_trip_through_emoji() {
    let text = "a\u{1F600}b";
    let to_b = lsp_position_to_offset(text, LspPosition { line: 0, character: 3 });
    assert_eq!(to_b, 5);
    let range = offsets_to_lsp_range(text, 1, 5);
    assert_eq!((range.start.line, range.start.character), (0, 1));
    assert_eq!((range.end.line, range.end.character), (0, 3));
    let back = lsp_position_to_offset(text, range.end);
    assert_eq!(back, 5);
}

#[test]
fn diagnostic_data_keeps_everything_the_json_channel_needs() {
    let uri = "file:///data.tsx";
    let source = "const el = <div><span children={1}>x</span></div>;";
    let diagnostics = compile_single(source, uri, &config());
    let diagnostic = find_by_code(&diagnostics, "CHILDREN_PROP_IGNORED").expect("children warns");
    let value = diagnostic_data(&diagnostic);
    assert_eq!(value["file"], serde_json::Value::String(diagnostic.file.clone()));
    assert_eq!(value["rendered"], serde_json::Value::String(diagnostic.rendered.clone()));
    assert!(value["fixes"].as_array().unwrap().len() == diagnostic.fixes.len());
    assert!(value["docs"].as_str().unwrap().ends_with("#children_prop_ignored"));
}

#[test]
fn specifier_resolution_covers_extensions_and_index_files() {
    let ids = vec![
        "file:///a/b.tsx".to_string(),
        "file:///a/counter.tsx".to_string(),
        "file:///a/nested/index.tsx".to_string(),
    ];
    assert_eq!(
        resolve_specifier("file:///a/b.tsx", "./counter", &ids),
        Some("file:///a/counter.tsx".to_string())
    );
    assert_eq!(
        resolve_specifier("file:///a/b.tsx", "./nested", &ids),
        Some("file:///a/nested/index.tsx".to_string())
    );
    assert_eq!(resolve_specifier("file:///a/b.tsx", "reze-js", &ids), None);
    assert_eq!(resolve_specifier("file:///a/b.tsx", "./missing", &ids), None);
}

#[test]
fn workspace_scan_skips_dependencies_and_dotfiles() {
    let root = std::env::temp_dir().join(format!(
        "reze-lsp-scan-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let src = root.join("src");
    let deps = root.join("node_modules").join("pkg");
    let hidden = root.join(".git");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&deps).unwrap();
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(src.join("app.tsx"), "const el = <div />;").unwrap();
    std::fs::write(src.join("note.txt"), "plain").unwrap();
    std::fs::write(deps.join("dep.ts"), "export const x = 1;").unwrap();
    std::fs::write(hidden.join("hook.ts"), "export const y = 1;").unwrap();
    let found = scan_workspace(&root);
    let uris: Vec<String> = found.iter().map(|(uri, _)| uri.clone()).collect();
    let app = path_to_uri(&src.join("app.tsx"));
    assert!(uris.contains(&app));
    assert!(!uris.iter().any(|uri| uri.contains("node_modules")));
    assert!(!uris.iter().any(|uri| uri.contains(".git")));
    assert!(!uris.iter().any(|uri| uri.ends_with(".txt")));
    std::fs::remove_dir_all(&root).unwrap();
}
