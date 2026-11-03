//! Compiler diagnostics with quick fixes over the Language Server Protocol.

mod convert;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use convert::Document;
use lsp_server::{Connection, Message, Request, Response};
use lsp_types::notification::Notification as NotificationMethod;
use lsp_types::request::Request as RequestMethod;
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    PositionEncodingKind, PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, notification as notify, request as req,
};
use reze_compiler::{DiskFs, FileSystem, LineIndex, Options, compile_project};

pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// Serves `connection` until shutdown, exit, or disconnect.
pub fn run(connection: Connection) -> Result<(), Box<dyn std::error::Error>> {
    connection.initialize(serde_json::to_value(capabilities())?)?;
    let mut documents = HashMap::new();
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                respond(&connection, request, &documents)?;
            }
            Message::Notification(notification) => {
                if notification.method == notify::Exit::METHOD {
                    return Ok(());
                }
                receive(&connection, &mut documents, notification)?;
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn respond(
    connection: &Connection,
    request: Request,
    documents: &HashMap<String, Document>,
) -> Result<(), Box<dyn std::error::Error>> {
    if request.method != req::CodeActionRequest::METHOD {
        return send(
            connection,
            Response::new_err(
                request.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                format!("unsupported method {}", request.method),
            ),
        );
    }
    let Request { id, params, .. } = request;
    let params: CodeActionParams = match serde_json::from_value(params) {
        Ok(params) => params,
        Err(error) => {
            return send(
                connection,
                Response::new_err(
                    id,
                    lsp_server::ErrorCode::InvalidParams as i32,
                    error.to_string(),
                ),
            );
        }
    };
    let actions = documents.get(params.text_document.uri.as_str()).map_or(Vec::new(), |document| {
        convert::code_actions(
            &params.text_document.uri,
            &document.text,
            &document.index,
            &document.reports,
            params.range,
            params.context.only.clone(),
        )
    });
    send(connection, Response::new_ok(id, actions))
}

fn receive(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    notification: lsp_server::Notification,
) -> Result<(), Box<dyn std::error::Error>> {
    let changed = match notification.method.as_str() {
        notify::DidOpenTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
            else {
                return Ok(());
            };
            let item = params.text_document;
            documents
                .insert(item.uri.to_string(), document(item.uri.clone(), item.text, item.version));
            item.uri
        }
        notify::DidChangeTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
            else {
                return Ok(());
            };
            let uri = params.text_document.uri;
            let Some(change) = params.content_changes.into_iter().last() else { return Ok(()) };
            if !documents.contains_key(uri.as_str()) {
                return Ok(());
            }
            documents.insert(
                uri.to_string(),
                document(uri.clone(), change.text, params.text_document.version),
            );
            uri
        }
        notify::DidCloseTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<DidCloseTextDocumentParams>(notification.params)
            else {
                return Ok(());
            };
            let uri = params.text_document.uri;
            if documents.remove(uri.as_str()).is_none() {
                return Ok(());
            }
            clear(connection, &uri)?;
            uri
        }
        _ => return Ok(()),
    };
    let previous: HashMap<String, Vec<String>> =
        documents.iter().map(|(key, document)| (key.clone(), rendered(document))).collect();
    compile(documents);
    for (key, document) in documents.iter() {
        if document.uri == changed || previous.get(key) != Some(&rendered(document)) {
            publish(connection, document)?;
        }
    }
    Ok(())
}

fn document(uri: Uri, text: String, version: i32) -> Document {
    Document { uri, index: LineIndex::new(&text), text, version, reports: Vec::new() }
}

fn rendered(document: &Document) -> Vec<String> {
    document.reports.iter().map(|report| report.rendered.clone()).collect()
}

/// The directory of the nearest `package.json` above `path`, or `None` when the file is not on
/// disk under a package.
fn project_root(path: &Path) -> Option<PathBuf> {
    path.ancestors().skip(1).find(|dir| dir.join("package.json").is_file()).map(Path::to_path_buf)
}

/// Open documents over the files on disk.
struct Overlay {
    disk: Option<DiskFs>,
    open: BTreeMap<String, String>,
}

impl FileSystem for Overlay {
    fn read(&self, path: &str) -> Option<String> {
        self.open.get(path).cloned().or_else(|| self.disk.as_ref()?.read(path))
    }

    fn list(&self, dir: &str) -> Vec<String> {
        let mut paths = self.disk.as_ref().map_or(Vec::new(), |disk| disk.list(dir));
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        paths.extend(
            self.open.keys().filter(|path| dir.is_empty() || path.starts_with(&prefix)).cloned(),
        );
        paths.sort();
        paths.dedup();
        paths
    }
}

/// Compiles every project with an open document and stores each document's diagnostics.
fn compile(documents: &mut HashMap<String, Document>) {
    let mut projects: BTreeMap<Option<PathBuf>, Vec<(String, String)>> = BTreeMap::new();
    for (key, document) in documents.iter() {
        let path = PathBuf::from(convert::filename(&document.uri));
        let root = project_root(&path);
        let relative = match &root {
            Some(root) => path.strip_prefix(root).unwrap_or(&path),
            None => path.strip_prefix("/").unwrap_or(&path),
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        projects.entry(root).or_default().push((key.clone(), relative));
    }
    for (root, members) in projects {
        let overlay = Overlay {
            disk: root.map(DiskFs::new),
            open: members
                .iter()
                .map(|(key, relative)| (relative.clone(), documents[key].text.clone()))
                .collect(),
        };
        let mut output = compile_project(&overlay, &Options::default());
        for (key, relative) in members {
            let document = documents.get_mut(&key).expect("member of an open document");
            document.reports = output
                .diagnostics
                .extract_if(.., |diagnostic| diagnostic.file == relative)
                .collect();
        }
    }
}

fn publish(connection: &Connection, document: &Document) -> Result<(), Box<dyn std::error::Error>> {
    let uri = &document.uri;
    send(
        connection,
        lsp_server::Notification::new(
            notify::PublishDiagnostics::METHOD.to_string(),
            PublishDiagnosticsParams {
                uri: uri.clone(),
                diagnostics: document
                    .reports
                    .iter()
                    .map(|report| convert::diagnostic(uri, &document.text, &document.index, report))
                    .collect(),
                version: Some(document.version),
            },
        ),
    )
}

fn clear(connection: &Connection, uri: &Uri) -> Result<(), Box<dyn std::error::Error>> {
    send(
        connection,
        lsp_server::Notification::new(
            notify::PublishDiagnostics::METHOD.to_string(),
            PublishDiagnosticsParams { uri: uri.clone(), diagnostics: Vec::new(), version: None },
        ),
    )
}

fn send(
    connection: &Connection,
    message: impl Into<Message>,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(connection.sender.send(message.into())?)
}
