use std::thread;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::Notification as NotificationMethod;
use lsp_types::request::Request as RequestMethod;
use lsp_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, NumberOrString, Position, PublishDiagnosticsParams, Range,
    ServerCapabilities, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextEdit, Uri, VersionedTextDocumentIdentifier, notification as notify, request as req,
};
use serde_json::{Value, json};

const DUPLICATE_BAD: &str = "export const view = <a href=\"/a\" href={url()} />;\n";
const DUPLICATE_GOOD: &str = "export const view = <a href={url()} />;\n";
const EMOJI_BAD: &str = "export const view = <div title=\"\u{1F389}\" clas=\"box\" />;\n";
const EMOJI_GOOD: &str = "export const view = <div title=\"\u{1F389}\" class=\"box\" />;\n";
const PARSE_BAD: &str = "export const view = <div>;\n";

struct Client {
    connection: Connection,
    next_id: i32,
}

impl Client {
    fn request(&mut self, method: &str, params: Value) -> Response {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.connection
            .sender
            .send(Message::Request(Request { id: id.clone(), method: method.to_string(), params }))
            .unwrap();
        loop {
            match self.connection.receiver.recv_timeout(Duration::from_secs(10)).unwrap() {
                Message::Response(response) if response.id == id => return response,
                Message::Response(_) => {}
                message => panic!("expected response, got {message:?}"),
            }
        }
    }

    fn notify(&self, notification: Notification) {
        self.connection.sender.send(Message::Notification(notification)).unwrap();
    }

    fn diagnostics(&self) -> PublishDiagnosticsParams {
        match self.connection.receiver.recv_timeout(Duration::from_secs(10)).unwrap() {
            Message::Notification(note) => {
                assert_eq!(note.method, notify::PublishDiagnostics::METHOD);
                serde_json::from_value(note.params).unwrap()
            }
            message => panic!("expected diagnostics, got {message:?}"),
        }
    }
}

type Server = thread::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>;

fn start() -> (Client, Server) {
    let (server, client) = Connection::memory();
    let handle =
        thread::spawn(move || reze_lsp::run(server).map_err(|error| error.to_string().into()));
    (Client { connection: client, next_id: 0 }, handle)
}

fn parse_uri(text: &str) -> Uri {
    text.parse().unwrap()
}

fn has(diagnostics: &[Diagnostic], code: &str) -> bool {
    diagnostics.iter().any(|d| d.code == Some(NumberOrString::String(code.to_string())))
}

fn to_offset(text: &str, at: Position) -> usize {
    let mut line_start = 0;
    for _ in 0..at.line {
        line_start =
            text[line_start..].find('\n').map_or(text.len(), |found| line_start + found + 1);
    }
    let mut taken = 0;
    let mut units = 0;
    for c in text[line_start..].chars() {
        if c == '\n' || units >= at.character {
            break;
        }
        units += c.len_utf16() as u32;
        taken += c.len_utf8();
    }
    line_start + taken
}

fn apply(text: &str, edits: &[TextEdit]) -> String {
    let mut ordered = edits.to_vec();
    ordered.sort_by_key(|edit| (edit.range.start.line, edit.range.start.character));
    let mut fixed = text.to_string();
    for edit in ordered.into_iter().rev() {
        let start = to_offset(&fixed, edit.range.start);
        let end = to_offset(&fixed, edit.range.end);
        fixed.replace_range(start..end, &edit.new_text);
    }
    fixed
}

fn full_line() -> Range {
    Range { start: Position { line: 0, character: 0 }, end: Position { line: 0, character: 1000 } }
}

fn actions(client: &mut Client, uri: &Uri, only: Option<Vec<CodeActionKind>>) -> Vec<CodeAction> {
    let response = client.request(
        req::CodeActionRequest::METHOD,
        serde_json::to_value(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: full_line(),
            context: CodeActionContext { diagnostics: Vec::new(), only, trigger_kind: None },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .unwrap(),
    );
    assert!(response.response_result.is_ok());
    let actions: Vec<CodeActionOrCommand> =
        serde_json::from_value(response.response_result.unwrap()).unwrap();
    actions
        .into_iter()
        .map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => action,
            CodeActionOrCommand::Command(command) => panic!("expected action, got {command:?}"),
        })
        .collect()
}

#[test]
fn session_reports_diagnostics_and_fixes() {
    let (mut client, server) = start();
    let uri = parse_uri("file:///view.tsx");

    let response = client.request("initialize", json!({"capabilities": {}}));
    let capabilities: ServerCapabilities =
        serde_json::from_value(response.response_result.unwrap()["capabilities"].clone()).unwrap();
    assert_eq!(
        capabilities.code_action_provider,
        Some(lsp_types::CodeActionProviderCapability::Options(lsp_types::CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        }))
    );
    assert_eq!(
        capabilities.text_document_sync,
        Some(lsp_types::TextDocumentSyncCapability::Kind(lsp_types::TextDocumentSyncKind::FULL))
    );
    assert_eq!(capabilities.position_encoding, Some(lsp_types::PositionEncodingKind::UTF16));
    client.notify(Notification::new(notify::Initialized::METHOD.to_string(), json!({})));

    client.notify(Notification::new(
        notify::DidOpenTextDocument::METHOD.to_string(),
        serde_json::to_value(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: String::from("typescriptreact"),
                version: 1,
                text: DUPLICATE_BAD.to_string(),
            },
        })
        .unwrap(),
    ));
    let published = client.diagnostics();
    assert_eq!(published.version, Some(1));
    assert!(has(&published.diagnostics, "DUPLICATE_ATTRIBUTE"));
    let report = published
        .diagnostics
        .iter()
        .find(|d| has(std::slice::from_ref(d), "DUPLICATE_ATTRIBUTE"))
        .unwrap();
    assert_eq!(report.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(report.range.start.line, 0);
    assert!(report.related_information.as_ref().is_some_and(|labels| !labels.is_empty()));
    assert!(report.code_description.is_some());

    let fixes = actions(&mut client, &uri, None);
    assert!(!fixes.is_empty());
    assert_eq!(fixes[0].kind, Some(CodeActionKind::QUICKFIX));
    assert!(fixes[0].title.contains("earlier"));
    assert_eq!(fixes[0].is_preferred, Some(true));
    let edits = fixes[0].edit.as_ref().unwrap().changes.as_ref().unwrap().get(&uri).unwrap();
    assert_eq!(apply(DUPLICATE_BAD, edits), DUPLICATE_GOOD);

    let filtered = actions(&mut client, &uri, Some(vec![CodeActionKind::REFACTOR]));
    assert!(filtered.is_empty());

    client.notify(Notification::new(
        notify::DidChangeTextDocument::METHOD.to_string(),
        serde_json::to_value(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri: uri.clone(), version: 2 },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: DUPLICATE_GOOD.to_string(),
            }],
        })
        .unwrap(),
    ));
    let cleared = client.diagnostics();
    assert_eq!(cleared.version, Some(2));
    assert!(!has(&cleared.diagnostics, "DUPLICATE_ATTRIBUTE"));

    let emoji = parse_uri("file:///emoji%20view.tsx");
    client.notify(Notification::new(
        notify::DidOpenTextDocument::METHOD.to_string(),
        serde_json::to_value(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: emoji.clone(),
                language_id: String::from("typescriptreact"),
                version: 1,
                text: EMOJI_BAD.to_string(),
            },
        })
        .unwrap(),
    ));
    let published = client.diagnostics();
    assert!(has(&published.diagnostics, "UNKNOWN_ATTRIBUTE"));
    let fixes = actions(&mut client, &emoji, None);
    let edits = fixes[0].edit.as_ref().unwrap().changes.as_ref().unwrap().get(&emoji).unwrap();
    assert_eq!(apply(EMOJI_BAD, edits), EMOJI_GOOD);
    client.notify(Notification::new(
        notify::DidCloseTextDocument::METHOD.to_string(),
        serde_json::to_value(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: emoji },
        })
        .unwrap(),
    ));
    assert!(client.diagnostics().diagnostics.is_empty());

    client.notify(Notification::new(
        notify::DidChangeTextDocument::METHOD.to_string(),
        serde_json::to_value(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri: uri.clone(), version: 3 },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: PARSE_BAD.to_string(),
            }],
        })
        .unwrap(),
    ));
    let broken = client.diagnostics();
    let parse =
        broken.diagnostics.iter().find(|d| has(std::slice::from_ref(d), "PARSE_ERROR")).unwrap();
    assert_eq!(parse.severity, Some(DiagnosticSeverity::ERROR));
    assert!(actions(&mut client, &uri, None).is_empty());

    let response = client.request("textDocument/hover", json!({}));
    assert_eq!(response.response_result.unwrap_err().code, -32601);

    client.notify(Notification::new(
        notify::DidCloseTextDocument::METHOD.to_string(),
        serde_json::to_value(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        })
        .unwrap(),
    ));
    assert!(client.diagnostics().diagnostics.is_empty());

    let response = client.request("shutdown", Value::Null);
    assert!(response.response_result.is_ok());
    client.notify(Notification::new(notify::Exit::METHOD.to_string(), Value::Null));
    server.join().unwrap().unwrap();
}
