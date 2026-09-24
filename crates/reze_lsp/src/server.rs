use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionResponse, DiagnosticOptions,
    DiagnosticServerCapabilities, DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, FullDocumentDiagnosticReport,
    HoverOptions, HoverParams, InitializeParams, InlayHint, InlayHintParams, OneOf,
    PositionEncodingKind, PublishDiagnosticsParams, RelatedFullDocumentDiagnosticReport,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    WorkDoneProgressOptions,
};
use serde_json::Value;

use crate::analysis::{Config, analyze_program, compile_single};
use crate::mapping::{
    code_actions, compiler_range_to_lsp, diagnostic_to_lsp, explain, hover_at, inlay_hints,
};
use reze_compiler::Diagnostic;

struct OpenDoc {
    text: String,
    version: i32,
}

struct State {
    open: HashMap<String, OpenDoc>,
    analyzed: HashMap<String, Vec<Diagnostic>>,
    texts: HashMap<String, String>,
    config: Config,
    workspace_root: Option<PathBuf>,
}

impl State {
    fn new(config: Config, workspace_root: Option<PathBuf>) -> Self {
        Self {
            open: HashMap::new(),
            analyzed: HashMap::new(),
            texts: HashMap::new(),
            config,
            workspace_root,
        }
    }

    fn reanalyze(&mut self) {
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        if let Some(root) = self.workspace_root.clone() {
            for (uri, text) in scan_workspace(&root) {
                files.insert(uri, text);
            }
        }
        for (uri, doc) in &self.open {
            files.insert(uri.clone(), doc.text.clone());
        }
        self.texts = files.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if files.len() > 1 {
            let reports = analyze_program(&files, &self.config);
            self.analyzed = reports.into_iter().collect();
        } else if let Some((uri, doc)) = self.open.iter().next() {
            let single = compile_single(&doc.text, uri, &self.config);
            self.analyzed = HashMap::from([(uri.clone(), single)]);
        } else {
            self.analyzed.clear();
        }
        for uri in self.open.keys() {
            self.analyzed.entry(uri.clone()).or_default();
        }
    }

    fn others_for(&self, skip: &str) -> BTreeMap<String, String> {
        self.texts
            .iter()
            .filter(|(uri, _)| uri.as_str() != skip)
            .map(|(uri, text)| (uri.clone(), text.clone()))
            .collect()
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "reze-lsp: language server for Reze diagnostics, inlay hints and island explanations"
        );
        println!("usage: reze-lsp [--version] [--help]");
        println!("speaks LSP over stdio");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("reze-lsp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let (connection, io_threads) = Connection::stdio();
    let (id, params) = connection.initialize_start()?;
    let init: InitializeParams = serde_json::from_value(params)?;
    let config = config_from_init(&init);
    let workspace_root = root_from_init(&init);
    let capabilities = capabilities();
    connection.initialize_finish(id, serde_json::json!({ "capabilities": capabilities }))?;
    let mut state = State::new(config, workspace_root);
    main_loop(connection, &mut state)?;
    io_threads.join()?;
    Ok(())
}

fn config_from_init(init: &InitializeParams) -> Config {
    let mut config = Config::default();
    if let Some(options) = init.initialization_options.clone() {
        apply_options(&mut config, &options);
    }
    if let Some(root) = root_from_init(init) {
        config.root = root.to_string_lossy().into_owned();
    }
    config
}

fn apply_options(config: &mut Config, options: &Value) {
    if let Some(name) = options.get("moduleName").and_then(Value::as_str) {
        config.module_name = name.to_string();
    }
    if let Some(optimize) = options.get("optimize").and_then(Value::as_bool) {
        config.optimize = optimize;
    }
    if let Some(islands) = options.get("islands").and_then(Value::as_bool) {
        config.islands = islands;
    }
    if let Some(root) = options.get("root").and_then(Value::as_str) {
        config.root = root.to_string();
    }
}

#[allow(deprecated)]
fn root_from_init(init: &InitializeParams) -> Option<PathBuf> {
    if let Some(folder) = init.workspace_folders.clone().and_then(|mut folders| folders.pop())
        && let Some(path) = uri_to_path(folder.uri.as_str())
    {
        return Some(path);
    }
    if let Some(uri) = init.root_uri.clone()
        && let Some(path) = uri_to_path(uri.as_str())
    {
        return Some(path);
    }
    init.root_path.clone().map(PathBuf::from)
}

fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
            identifier: Some("reze".to_string()),
            inter_file_dependencies: true,
            workspace_diagnostics: false,
            work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
        })),
        inlay_hint_provider: Some(OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Options(HoverOptions {
            work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
        })),
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Options(
            CodeActionOptions {
                code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                resolve_provider: Some(false),
                work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
            },
        )),
        ..Default::default()
    }
}

fn main_loop(connection: Connection, state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                handle_request(&connection, state, request)?;
            }
            Message::Response(_) => {}
            Message::Notification(note) => {
                if handle_notification(&connection, state, note)? {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

fn handle_request(
    connection: &Connection,
    state: &mut State,
    request: Request,
) -> Result<(), Box<dyn std::error::Error>> {
    let Request { id, method, params } = request;
    let mut handled = true;
    match method.as_str() {
        "textDocument/diagnostic" => {
            let params: DocumentDiagnosticParams = serde_json::from_value(params)?;
            let uri = params.text_document.uri.as_str().to_string();
            let items = diagnostic_items(state, &uri);
            let report = RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items,
                },
            };
            let result =
                DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report));
            respond(connection, id.clone(), Ok(result))?;
        }
        "textDocument/inlayHint" => {
            let params: InlayHintParams = serde_json::from_value(params)?;
            let uri = params.text_document.uri.as_str().to_string();
            let hints = inlay_items(state, &uri, params.range);
            respond(connection, id.clone(), Ok(hints))?;
        }
        "textDocument/hover" => {
            let params: HoverParams = serde_json::from_value(params)?;
            let uri = params.text_document_position_params.text_document.uri.as_str().to_string();
            let hover = hover_for(state, &uri, params.text_document_position_params.position);
            respond(connection, id.clone(), Ok(hover))?;
        }
        "textDocument/codeAction" => {
            let params: CodeActionParams = serde_json::from_value(params)?;
            let uri = params.text_document.uri.as_str().to_string();
            let actions = actions_for(state, &uri, params.range);
            respond(connection, id.clone(), Ok(CodeActionResponse::from(actions)))?;
        }
        "reze/whyIsland" => {
            let params: WhyIslandParams = serde_json::from_value(params)?;
            let answer = why_island(state, &params.uri, params.offset);
            respond(connection, id.clone(), Ok(answer))?;
        }
        _ => {
            handled = false;
        }
    }
    if !handled {
        respond_empty(connection, id)?;
    }
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    state: &mut State,
    note: Notification,
) -> Result<bool, Box<dyn std::error::Error>> {
    match note.method.as_str() {
        "textDocument/didOpen" => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(note.params)?;
            let uri = params.text_document.uri.as_str().to_string();
            state.open.insert(
                uri,
                OpenDoc { text: params.text_document.text, version: params.text_document.version },
            );
            state.reanalyze();
            publish_all(connection, state);
        }
        "textDocument/didChange" => {
            let params: DidChangeTextDocumentParams = serde_json::from_value(note.params)?;
            let uri = params.text_document.uri.as_str().to_string();
            if let Some(change) = params.content_changes.into_iter().last() {
                state.open.insert(
                    uri,
                    OpenDoc { text: change.text, version: params.text_document.version },
                );
                state.reanalyze();
                publish_all(connection, state);
            }
        }
        "textDocument/didClose" => {
            let params: DidCloseTextDocumentParams = serde_json::from_value(note.params)?;
            let uri = params.text_document.uri.as_str().to_string();
            state.open.remove(&uri);
            state.analyzed.remove(&uri);
            state.reanalyze();
            publish_all(connection, state);
        }
        "workspace/didChangeConfiguration" => {
            let params: DidChangeConfigurationParams = serde_json::from_value(note.params)?;
            apply_options(&mut state.config, &params.settings);
            state.reanalyze();
            publish_all(connection, state);
        }
        "exit" => {
            return Ok(true);
        }
        _ => {}
    }
    Ok(false)
}

fn diagnostic_items(state: &State, uri: &str) -> Vec<lsp_types::Diagnostic> {
    let source = state.texts.get(uri).cloned().unwrap_or_default();
    let others = state.others_for(uri);
    state
        .analyzed
        .get(uri)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|diagnostic| diagnostic_to_lsp(uri, diagnostic, &source, &others))
        .collect()
}

fn inlay_items(state: &State, uri: &str, range: lsp_types::Range) -> Vec<InlayHint> {
    let diagnostics = state.analyzed.get(uri).cloned().unwrap_or_default();
    inlay_hints(&diagnostics)
        .into_iter()
        .filter(|hint| {
            hint.position.line < range.end.line
                || (hint.position.line == range.end.line
                    && hint.position.character <= range.end.character)
        })
        .filter(|hint| {
            hint.position.line > range.start.line
                || (hint.position.line == range.start.line
                    && hint.position.character >= range.start.character)
        })
        .collect()
}

fn hover_for(state: &State, uri: &str, position: lsp_types::Position) -> Option<lsp_types::Hover> {
    let source = state.texts.get(uri)?;
    let diagnostics = state.analyzed.get(uri)?;
    hover_at(source, diagnostics, position)
}

fn actions_for(
    state: &State,
    uri: &str,
    range: lsp_types::Range,
) -> Vec<lsp_types::CodeActionOrCommand> {
    let Some(source) = state.texts.get(uri) else { return Vec::new() };
    let diagnostics = state.analyzed.get(uri).cloned().unwrap_or_default();
    code_actions(uri, source, &diagnostics, range)
        .into_iter()
        .map(lsp_types::CodeActionOrCommand::CodeAction)
        .collect()
}

#[derive(serde::Deserialize)]
struct WhyIslandParams {
    uri: String,
    offset: u32,
}

#[derive(serde::Serialize)]
struct WhyIslandAnswer {
    markdown: Option<String>,
    range: Option<lsp_types::Range>,
}

fn why_island(state: &State, uri: &str, offset: u32) -> WhyIslandAnswer {
    let Some(diagnostics) = state.analyzed.get(uri) else {
        return WhyIslandAnswer { markdown: None, range: None };
    };
    let mut hits: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.start.offset <= offset && offset <= diagnostic.end.offset)
        .collect();
    hits.sort_by_key(|diagnostic| {
        (island_priority(diagnostic), diagnostic.end.offset - diagnostic.start.offset)
    });
    match hits.first() {
        Some(top) => WhyIslandAnswer {
            range: Some(compiler_range_to_lsp(top)),
            markdown: Some(explain(top)),
        },
        None => WhyIslandAnswer { markdown: None, range: None },
    }
}

fn island_priority(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.code.name() {
        "ISLAND" | "LAZY_ISLAND" => 0,
        "CLIENT_COMPONENT" | "STATIC_COMPONENT" => 1,
        _ => 2,
    }
}

fn respond<T: serde::Serialize>(
    connection: &Connection,
    id: RequestId,
    result: Result<T, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = match result {
        Ok(value) => Response { id, response_result: Ok(serde_json::to_value(value)?) },
        Err(message) => Response {
            id,
            response_result: Err(lsp_server::ResponseError {
                code: lsp_server::ErrorCode::RequestFailed as i32,
                message,
                data: None,
            }),
        },
    };
    connection.sender.send(Message::Response(response))?;
    Ok(())
}

fn respond_empty(connection: &Connection, id: RequestId) -> Result<(), Box<dyn std::error::Error>> {
    let response = Response { id, response_result: Ok(Value::Null) };
    connection.sender.send(Message::Response(response))?;
    Ok(())
}

fn publish_all(connection: &Connection, state: &State) {
    for (uri, doc) in &state.open {
        let Ok(uri_parsed) = uri.parse::<Uri>() else { continue };
        let source = state.texts.get(uri).cloned().unwrap_or_default();
        let others = state.others_for(uri);
        let diagnostics: Vec<lsp_types::Diagnostic> = state
            .analyzed
            .get(uri)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|diagnostic| diagnostic_to_lsp(uri, diagnostic, &source, &others))
            .collect();
        let params =
            PublishDiagnosticsParams { uri: uri_parsed, diagnostics, version: Some(doc.version) };
        let note = Notification {
            method: "textDocument/publishDiagnostics".to_string(),
            params: serde_json::to_value(params).unwrap_or(Value::Null),
        };
        let _ = connection.sender.send(Message::Notification(note));
    }
}

const SOURCE_EXTENSIONS: [&str; 8] = [".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"];

pub fn scan_workspace(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if name == "node_modules" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !SOURCE_EXTENSIONS.iter().any(|ext| name.ends_with(ext)) {
                continue;
            }
            if out.len() >= 5000 {
                return out;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((path_to_uri(&path), text));
            }
        }
    }
    out
}

pub fn path_to_uri(path: &Path) -> String {
    let mut absolute = path.to_path_buf();
    if absolute.is_relative()
        && let Ok(cwd) = std::env::current_dir()
    {
        absolute = cwd.join(absolute);
    }
    let mut text = absolute.to_string_lossy().replace('\\', "/");
    if !text.starts_with('/') {
        text.insert(0, '/');
    }
    format!("file://{text}")
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.split('?').next().unwrap_or(rest);
    Some(PathBuf::from(percent_decode_min(path)))
}

fn percent_decode_min(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
