use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, Message, Response};
use lsp_types::*;
use vivi_parser::ast::*;

/// Symbol definition: name → (file_uri, byte_offset_start, byte_offset_end)
struct SymbolTable {
    /// component/system/fn/entity names → definition span
    definitions: HashMap<String, (String, usize, usize)>,
    /// source text per file
    sources: HashMap<String, String>,
}

impl SymbolTable {
    fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            sources: HashMap::new(),
        }
    }

    fn index_file(&mut self, uri: &str, source: &str) {
        self.sources.insert(uri.to_string(), source.to_string());
        self.definitions.retain(|_, (u, _, _)| u != uri);

        let program = match vivi_parser::parse(source) {
            Ok(p) => p,
            Err(_) => return,
        };

        for item in &program.items {
            match item {
                Item::Component(c) => {
                    self.definitions.insert(c.name.clone(), (uri.to_string(), c.span.start, c.span.end));
                }
                Item::System(s) => {
                    self.definitions.insert(s.name.clone(), (uri.to_string(), s.span.start, s.span.end));
                }
                Item::Fn(f) => {
                    self.definitions.insert(f.name.clone(), (uri.to_string(), f.span.start, f.span.end));
                }
                Item::Entity(e) => {
                    self.definitions.insert(e.name.clone(), (uri.to_string(), e.span.start, e.span.end));
                }
                Item::World(w) => {
                    self.definitions.insert(w.name.clone(), (uri.to_string(), w.span.start, w.span.end));
                }
                Item::Extern(ext) => {
                    for f in &ext.functions {
                        self.definitions.insert(f.name.clone(), (uri.to_string(), f.span.start, f.span.end));
                    }
                }
                Item::Global(g) => {
                    self.definitions.insert(g.name.clone(), (uri.to_string(), g.span.start, g.span.end));
                }
            }
        }
    }

    fn offset_to_position(&self, uri: &str, offset: usize) -> Position {
        let source = match self.sources.get(uri) {
            Some(s) => s,
            None => return Position::new(0, 0),
        };
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in source.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        Position::new(line, col)
    }

    fn position_to_offset(&self, uri: &str, pos: Position) -> usize {
        let source = match self.sources.get(uri) {
            Some(s) => s,
            None => return 0,
        };
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in source.char_indices() {
            if line == pos.line && col == pos.character {
                return i;
            }
            if ch == '\n' {
                if line == pos.line {
                    return i;
                }
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        source.len()
    }

    fn word_at_offset(&self, uri: &str, offset: usize) -> Option<String> {
        let source = self.sources.get(uri)?;
        let bytes = source.as_bytes();
        if offset >= bytes.len() {
            return None;
        }
        let mut start = offset;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }
        let mut end = offset;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        if start == end {
            return None;
        }
        Some(String::from_utf8_lossy(&bytes[start..end]).to_string())
    }

    fn completions(&self) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        // Keywords
        for kw in &[
            "component", "system", "query", "read", "write", "each", "world",
            "entity", "extern", "fn", "if", "else", "while", "let", "return",
            "spawn", "despawn", "true", "false", "and", "or", "not", "init", "systems",
        ] {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        // Types
        for ty in &["i32", "i64", "f32", "f64", "bool", "Entity"] {
            items.push(CompletionItem {
                label: ty.to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                ..Default::default()
            });
        }

        // User-defined symbols (components, systems, functions, entities)
        for (name, _) in &self.definitions {
            let kind = if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                CompletionItemKind::CLASS
            } else {
                CompletionItemKind::FUNCTION
            };
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(kind),
                ..Default::default()
            });
        }

        items
    }

    fn find_definition(&self, word: &str) -> Option<Location> {
        let (uri, start, _end) = self.definitions.get(word)?;
        let pos = self.offset_to_position(uri, *start);
        Some(Location {
            uri: uri.parse().ok()?,
            range: Range::new(pos, pos),
        })
    }
}

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), " ".into()]),
            ..Default::default()
        }),
        ..Default::default()
    })?;

    let init_params = match connection.initialize(server_capabilities) {
        Ok(it) => it,
        Err(e) => {
            eprintln!("LSP init failed: {e}");
            return Ok(());
        }
    };

    let _init: InitializeParams = serde_json::from_value(init_params)?;
    let mut symbols = SymbolTable::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }

                if req.method == "textDocument/definition" {
                    let params: GotoDefinitionParams = serde_json::from_value(req.params)?;
                    let uri = params.text_document_position_params.text_document.uri.to_string();
                    let pos = params.text_document_position_params.position;
                    let offset = symbols.position_to_offset(&uri, pos);

                    let result = symbols
                        .word_at_offset(&uri, offset)
                        .and_then(|word| symbols.find_definition(&word));

                    let resp = Response::new_ok(
                        req.id,
                        result.map(GotoDefinitionResponse::Scalar),
                    );
                    connection.sender.send(Message::Response(resp))?;
                } else if req.method == "textDocument/hover" {
                    let params: HoverParams = serde_json::from_value(req.params)?;
                    let uri = params.text_document_position_params.text_document.uri.to_string();
                    let pos = params.text_document_position_params.position;
                    let offset = symbols.position_to_offset(&uri, pos);
                    let hover = symbols.word_at_offset(&uri, offset).and_then(|word| {
                        let (def_uri, start, end) = symbols.definitions.get(&word)?;
                        let source = symbols.sources.get(def_uri)?;
                        let snippet: String = source[*start..*end.min(&source.len())]
                            .chars()
                            .take(200)
                            .collect();
                        let first_line = snippet.lines().next().unwrap_or(&snippet);
                        Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!("```vivi\n{first_line}\n```"),
                            }),
                            range: None,
                        })
                    });

                    let resp = Response::new_ok(req.id, hover);
                    connection.sender.send(Message::Response(resp))?;
                } else if req.method == "textDocument/completion" {
                    let items = symbols.completions();
                    let resp = Response::new_ok(req.id, Some(items));
                    connection.sender.send(Message::Response(resp))?;
                } else {
                    let resp = Response::new_err(
                        req.id,
                        lsp_server::ErrorCode::MethodNotFound as i32,
                        format!("unhandled method: {}", req.method),
                    );
                    connection.sender.send(Message::Response(resp))?;
                }
            }
            Message::Notification(notif) => {
                if notif.method == "textDocument/didOpen" {
                    let params: DidOpenTextDocumentParams =
                        serde_json::from_value(notif.params)?;
                    let uri = params.text_document.uri.to_string();
                    symbols.index_file(&uri, &params.text_document.text);
                } else if notif.method == "textDocument/didChange" {
                    let params: DidChangeTextDocumentParams =
                        serde_json::from_value(notif.params)?;
                    let uri = params.text_document.uri.to_string();
                    if let Some(change) = params.content_changes.into_iter().last() {
                        symbols.index_file(&uri, &change.text);
                    }
                }
            }
            Message::Response(_) => {}
        }
    }

    io_threads.join()?;
    Ok(())
}
