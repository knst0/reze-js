//! Compiler diagnostics with quick fixes over the Language Server Protocol.

mod convert;

use std::collections::HashMap;

use convert::Document;
use lsp_server::{Connection, Message, Request, Response};
use lsp_types::notification::Notification as NotificationMethod;
use lsp_types::request::Request as RequestMethod;
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GotoDefinitionParams, Hover, HoverContents, HoverParams, HoverProviderCapability, Location,
    MarkupContent, MarkupKind, OneOf, PositionEncodingKind, PublishDiagnosticsParams,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    notification as notify, request as req,
};
use reze_compiler::{LineIndex, Options};

pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        })),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
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
    match request.method.as_str() {
        req::CodeActionRequest::METHOD => {
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
            let actions =
                documents.get(params.text_document.uri.as_str()).map_or(Vec::new(), |document| {
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
        req::GotoDefinition::METHOD => {
            let Request { id, params, .. } = request;
            let Ok(params) = serde_json::from_value::<GotoDefinitionParams>(params) else {
                return send(
                    connection,
                    Response::new_err(
                        id,
                        lsp_server::ErrorCode::InvalidParams as i32,
                        String::from("invalid definition params"),
                    ),
                );
            };
            let at = params.text_document_position_params;
            let location = documents.get(at.text_document.uri.as_str()).and_then(|document| {
                let offset = document.offset_at(at.position)?;
                let inspection = reze_compiler::inspect(
                    &document.text,
                    &convert::filename(&at.text_document.uri),
                    offset,
                )?;
                Some(Location {
                    uri: at.text_document.uri.clone(),
                    range: document
                        .span_range(inspection.declaration_start, inspection.declaration_end),
                })
            });
            send(connection, Response::new_ok(id, location))
        }
        req::HoverRequest::METHOD => {
            let Request { id, params, .. } = request;
            let Ok(params) = serde_json::from_value::<HoverParams>(params) else {
                return send(
                    connection,
                    Response::new_err(
                        id,
                        lsp_server::ErrorCode::InvalidParams as i32,
                        String::from("invalid hover params"),
                    ),
                );
            };
            let at = params.text_document_position_params;
            let hover = documents.get(at.text_document.uri.as_str()).and_then(|document| {
                let offset = document.offset_at(at.position)?;
                let inspection = reze_compiler::inspect(
                    &document.text,
                    &convert::filename(&at.text_document.uri),
                    offset,
                )?;
                Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!(
                            "**{}** `{}`\n\n```tsx\n{}\n```",
                            inspection.kind.label(),
                            inspection.name,
                            inspection.lowered
                        ),
                    }),
                    range: Some(document.span_range(inspection.use_start, inspection.use_end)),
                })
            });
            send(connection, Response::new_ok(id, hover))
        }
        _ => send(
            connection,
            Response::new_err(
                request.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                format!("unsupported method {}", request.method),
            ),
        ),
    }
}

fn receive(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    notification: lsp_server::Notification,
) -> Result<(), Box<dyn std::error::Error>> {
    match notification.method.as_str() {
        notify::DidOpenTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
            else {
                return Ok(());
            };
            let document = compile(
                &params.text_document.uri,
                &params.text_document.text,
                params.text_document.version,
            );
            publish(connection, &params.text_document.uri, &document)?;
            documents.insert(params.text_document.uri.as_str().to_string(), document);
        }
        notify::DidChangeTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
            else {
                return Ok(());
            };
            let Some(document) = documents.get_mut(params.text_document.uri.as_str()) else {
                return Ok(());
            };
            if let Some(change) = params.content_changes.into_iter().last() {
                *document =
                    compile(&params.text_document.uri, &change.text, params.text_document.version);
                publish(connection, &params.text_document.uri, document)?;
            }
        }
        notify::DidCloseTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<DidCloseTextDocumentParams>(notification.params)
            else {
                return Ok(());
            };
            if documents.remove(params.text_document.uri.as_str()).is_some() {
                clear(connection, &params.text_document.uri)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn compile(uri: &Uri, text: &str, version: i32) -> Document {
    let filename = convert::filename(uri);
    let options = Options::default();
    let reports = match reze_compiler::compile(text, &filename, &options) {
        Ok(output) => output.map(|output| output.diagnostics).unwrap_or_default(),
        Err(reports) => reports,
    };
    Document { text: text.to_string(), version, index: LineIndex::new(text), reports }
}

fn publish(
    connection: &Connection,
    uri: &Uri,
    document: &Document,
) -> Result<(), Box<dyn std::error::Error>> {
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
