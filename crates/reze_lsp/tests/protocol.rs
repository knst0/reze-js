use std::collections::HashMap;
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

const CONDITIONAL: &str = "export default function Home() {\n  const ok = true;\n  return <main>{ok && <b>yes</b>}</main>;\n}\n";
const CONDITIONAL_FIXED: &str = "import { Show } from \"reze-js\";\nexport default function Home() {\n  const ok = true;\n  return <main><Show when={ok}><b>yes</b></Show></main>;\n}\n";
const IMPORTS_CARD: &str = "import Card from \"../Card\";\nexport default function Home() {\n  return <main><Card /></main>;\n}\n";
const CARD: &str = "export default function Card() {\n  const items = [\"a\"];\n  return <ul>{items.map((item) => <li>{item}</li>)}</ul>;\n}\n";
const PARSE_BAD: &str = "export default function Home() {\n  return <main>;\n}\n";

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

    /// The next `count` publications, by document.
    fn publications(&self, count: usize) -> HashMap<String, Vec<Diagnostic>> {
        (0..count).map(|_| self.diagnostics()).map(|p| (p.uri.to_string(), p.diagnostics)).collect()
    }

    fn quiet(&self) {
        assert!(self.connection.receiver.recv_timeout(Duration::from_millis(200)).is_err());
    }

    fn open(&self, uri: &Uri, text: &str) {
        self.notify(Notification::new(
            notify::DidOpenTextDocument::METHOD.to_string(),
            serde_json::to_value(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: String::from("typescriptreact"),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .unwrap(),
        ));
    }

    fn change(&self, uri: &Uri, version: i32, text: &str) {
        self.notify(Notification::new(
            notify::DidChangeTextDocument::METHOD.to_string(),
            serde_json::to_value(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier { uri: uri.clone(), version },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_string(),
                }],
            })
            .unwrap(),
        ));
    }

    fn close(&self, uri: &Uri) {
        self.notify(Notification::new(
            notify::DidCloseTextDocument::METHOD.to_string(),
            serde_json::to_value(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            })
            .unwrap(),
        ));
    }

    fn actions(&mut self, uri: &Uri, only: Option<Vec<CodeActionKind>>) -> Vec<CodeAction> {
        let response = self.request(
            req::CodeActionRequest::METHOD,
            serde_json::to_value(CodeActionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                range: Range {
                    start: Position { line: 0, character: 0 },
                    end: Position { line: 100, character: 0 },
                },
                context: CodeActionContext { diagnostics: Vec::new(), only, trigger_kind: None },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .unwrap(),
        );
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

fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|d| match &d.code {
            Some(NumberOrString::String(code)) => Some(code.clone()),
            _ => None,
        })
        .collect()
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

#[test]
fn session_reports_project_diagnostics_and_fixes() {
    let (mut client, server) = start();
    let page = parse_uri("file:///src/routes/index.tsx");
    let card = parse_uri("file:///src/Card.tsx");

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

    client.open(&page, CONDITIONAL);
    let published = client.diagnostics();
    assert_eq!(published.uri, page);
    assert_eq!(published.version, Some(1));
    assert_eq!(codes(&published.diagnostics), ["JSX_CONDITIONAL_EXPRESSION"]);
    let report = &published.diagnostics[0];
    assert_eq!(report.range.start, Position { line: 2, character: 16 });
    assert!(report.code_description.is_some());

    let fixes = client.actions(&page, None);
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(fixes[0].is_preferred, Some(true));
    let edits = fixes[0].edit.as_ref().unwrap().changes.as_ref().unwrap().get(&page).unwrap();
    assert_eq!(apply(CONDITIONAL, edits), CONDITIONAL_FIXED);
    assert!(client.actions(&page, Some(vec![CodeActionKind::REFACTOR])).is_empty());

    client.change(&page, 2, CONDITIONAL_FIXED);
    let cleared = client.diagnostics();
    assert_eq!(cleared.version, Some(2));
    assert!(cleared.diagnostics.is_empty());

    client.change(&page, 3, IMPORTS_CARD);
    let missing = client.diagnostics();
    assert_eq!(codes(&missing.diagnostics), ["IMPORT_UNRESOLVED"]);

    client.open(&card, CARD);
    let reached = client.publications(2);
    assert!(reached[page.as_str()].is_empty());
    assert_eq!(codes(&reached[card.as_str()]), ["JSX_LIST_EXPRESSION"]);

    client.change(&page, 4, PARSE_BAD);
    let broken = client.publications(2);
    let parse = &broken[page.as_str()][0];
    assert_eq!(codes(std::slice::from_ref(parse)), ["PARSE_ERROR"]);
    assert_eq!(parse.severity, Some(DiagnosticSeverity::ERROR));
    assert!(broken[card.as_str()].is_empty());
    assert!(client.actions(&page, None).is_empty());

    client.close(&card);
    assert!(client.diagnostics().diagnostics.is_empty());
    client.quiet();
    client.close(&page);
    assert!(client.diagnostics().diagnostics.is_empty());

    let response = client.request("shutdown", Value::Null);
    assert!(response.response_result.is_ok());
    client.notify(Notification::new(notify::Exit::METHOD.to_string(), Value::Null));
    server.join().unwrap().unwrap();
}
