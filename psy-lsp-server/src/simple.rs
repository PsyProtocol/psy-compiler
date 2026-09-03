use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    vec,
};

use dargo::resolve_crate_path_graph;
use psy_ast::{Position, Program, TextPosition, TextRange};
use psy_common::{FileId, Graph};
use psy_interpreter::Interpreter;
use psy_package::{
    files::{find_file_manifest_root, get_package_manifest},
    resolve_workspace_from_toml,
};
use psy_sema::{offset_from_position, TypeCheckError, TypeCheckerVisitorContext};
use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};
use tower_lsp::{
    jsonrpc::{Error as TError, Result as TResult},
    lsp_types::{
        CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity, DidChangeConfigurationParams, DidChangeTextDocumentParams,
        DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        DidSaveTextDocumentParams, DocumentFormattingParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
        InitializeParams, InitializeResult, InitializedParams, Location, MarkupContent, MarkupKind, OneOf, Range, ReferenceParams,
        TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Url,
    },
    Client, LanguageServer,
};

use crate::{
    error::{QLspError, QLspResult},
    utils::span_to_range,
};

#[derive(Debug, Clone)]
pub struct DiagnosticBundle {
    pub uri: Option<Url>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct QLspSimple {
    client: Client,
    ctx: Arc<RwLock<TypeCheckerVisitorContext<SymFeltRef, QExecContext>>>,
    root_path: Arc<RwLock<PathBuf>>,
    crate_path_graph_cache: Arc<RwLock<HashMap<PathBuf, Graph<PathBuf>>>>,
    last_diagnostics: Arc<RwLock<Option<DiagnosticBundle>>>,
}

impl QLspSimple {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            ctx: Arc::new(RwLock::new(TypeCheckerVisitorContext::new(Program::new()))),
            root_path: Arc::new(RwLock::new(PathBuf::new())),
            crate_path_graph_cache: Arc::new(RwLock::new(HashMap::new())),
            last_diagnostics: Arc::new(RwLock::new(None)),
        }
    }

    pub fn get_ctx_read(&self) -> RwLockReadGuard<'_, TypeCheckerVisitorContext<SymFeltRef, QExecContext>> {
        self.ctx.read().expect("ctx read lock poisoned")
    }

    pub fn get_ctx_write(&self) -> RwLockWriteGuard<'_, TypeCheckerVisitorContext<SymFeltRef, QExecContext>> {
        self.ctx.write().expect("ctx write lock poisoned")
    }

    pub fn get_root_path(&self) -> PathBuf {
        self.root_path.read().expect("root_path read lock poisoned").clone()
    }
    pub fn root_uri(&self) -> Option<Url> {
        let path = self.get_root_path();
        Url::from_file_path(path).ok()
    }
    pub fn client(&self) -> &Client {
        &self.client
    }
    pub fn set_root_path(&self, path: &PathBuf) -> QLspResult<()> {
        let mut locked = self
            .root_path
            .write()
            .map_err(|e| QLspError::Internal(format!("Failed to lock root_path (poisoned): {}", e)))?;
        *locked = path.clone();
        Ok(())
    }
    pub fn set_ctx(&self, new_ctx: TypeCheckerVisitorContext<SymFeltRef, QExecContext>) -> QLspResult<()> {
        let mut ctx = self.get_ctx_write();
        *ctx = new_ctx;
        Ok(())
    }

    pub fn get_cached_crate_path_graph(&self, path: &PathBuf) -> Option<Graph<PathBuf>> {
        self.crate_path_graph_cache.read().ok().and_then(|map| map.get(path).cloned())
    }
    pub fn set_crate_path_graph_cache(&self, path: PathBuf, graph: Graph<PathBuf>) {
        let mut map = self.crate_path_graph_cache.write().expect("lock poisoned");
        map.insert(path, graph);
    }
    pub fn clear_all_crate_path_graph_cache(&self) {
        let mut map = self.crate_path_graph_cache.write().expect("lock poisoned");
        map.clear();
    }
    pub fn remove_crate_path_graph_cache(&self, path: &PathBuf) {
        let mut map = self.crate_path_graph_cache.write().expect("lock poisoned");
        map.remove(path);
    }

    pub fn set_last_diagnostics(&self, bundle: DiagnosticBundle) {
        let mut guard = self.last_diagnostics.write().expect("lock poisoned");
        *guard = Some(bundle);
    }

    pub fn get_last_diagnostics(&self) -> Option<DiagnosticBundle> {
        self.last_diagnostics.read().ok().and_then(|guard| guard.clone())
    }

    pub fn clear_last_diagnostics(&self) {
        let mut guard = self.last_diagnostics.write().expect("lock poisoned");
        *guard = None;
    }
    pub async fn maybe_publish_cached_diagnostics(&self, uri: &Url) {
        if let Some(bundle) = self.get_last_diagnostics() {
            if let Some(bundle_uri) = &bundle.uri {
                if bundle_uri == uri {
                    eprintln!("[diagnostics] Re-publishing cached diagnostics for {:?}", uri);
                    self.client.publish_diagnostics(uri.clone(), bundle.diagnostics, None).await;
                }
            }
        }
    }
    pub async fn clear_cached_diagnostics(&self) {
        let uri_opt = self.get_last_diagnostics().and_then(|bundle| bundle.uri.clone());

        if uri_opt.is_some() {
            self.clear_last_diagnostics();
        }

        if let Some(uri) = uri_opt {
            eprintln!("[diagnostics] Clearing cached diagnostics for {:?}", uri);
            self.client.publish_diagnostics(uri, vec![], None).await;
        }
    }
    pub fn is_ready(&self) -> bool {
        let ctx = self.get_ctx_read();
        !ctx.symbols.is_empty()
    }

    pub fn resolve_file_id(&self, path: &PathBuf) -> QLspResult<FileId> {
        let ctx_guard = self.get_ctx_read();
        ctx_guard
            .program
            .file_resolver
            .resolve_id(path)
            .ok_or_else(|| QLspError::FileIdNotFound(path.clone()))
    }

    pub async fn init_and_publish_diagnostics(&self, root_path: &PathBuf) -> TResult<()> {
        match self.collect_diagnostics_sync(root_path) {
            Ok(bundle) => {
                eprintln!("[diagnostics] Start publishing diagnostics");
                eprintln!("[diagnostics] URI: {:?}", bundle.uri);
                eprintln!("[diagnostics] Payload: {:?}", bundle.diagnostics);

                if let Some(uri) = &bundle.uri {
                    self.client.publish_diagnostics(uri.clone(), bundle.diagnostics.clone(), None).await;
                    self.set_last_diagnostics(bundle.clone());

                    eprintln!("[diagnostics] Publish finished");
                } else {
                    let _ = self.clear_cached_diagnostics().await;
                    eprintln!("[diagnostics] No URI in bundle, skipping publish and clearing cache");
                }
            }
            Err(err) => {
                eprintln!("[diagnostics] Failed to collect diagnostics: {:?}", err);

                if let Some(uri) = self.get_last_diagnostics().and_then(|bundle| bundle.uri.clone()) {
                    self.client.publish_diagnostics(uri, vec![], None).await;
                    self.clear_last_diagnostics();
                    eprintln!("[diagnostics] Cleared previous diagnostics due to failure");
                }

                let _ = self
                    .client
                    .log_message(tower_lsp::lsp_types::MessageType::ERROR, format!("Context error: {}", err))
                    .await;
            }
        }

        Ok(())
    }

    pub fn collect_diagnostics_sync(&self, root_path: &PathBuf) -> QLspResult<DiagnosticBundle> {
        self.set_root_path(root_path)?;

        //use cached crate graph if available
        let crate_path_graph = if let Some(cached) = self.get_cached_crate_path_graph(root_path) {
            dbg!("Using cached crate graph");
            cached
        } else {
            dbg!("Cannot find cached crate graph, creating a new one");

            let package_dir = find_file_manifest_root(&root_path).map_err(|e| QLspError::Internal(format!("Failed to find manifest root: {}", e)))?;

            let toml_path = get_package_manifest(&package_dir).map_err(|e| QLspError::Internal(format!("Failed to get manifest file: {}", e)))?;

            // Resolve the workspace from the toml file. It will download dependencies as
            // well.
            let workspace =
                resolve_workspace_from_toml(&toml_path).map_err(|e| QLspError::Internal(format!("Failed to resolve workspace: {}", e)))?;

            let crate_path_graph = resolve_crate_path_graph(&workspace, None);

            self.set_crate_path_graph_cache(root_path.clone(), crate_path_graph.clone());
            crate_path_graph
        };

        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());

        let result = interpreter.typecheck_lsp(crate_path_graph);

        match result {
            Ok((_typechecker, ctx)) => {
                eprintln!("typecheck_lsp success");

                if let Err(e) = self.set_ctx(ctx) {
                    eprintln!("[init warning] Failed to set ctx: {}", e);
                    let _ = self
                        .client
                        .log_message(tower_lsp::lsp_types::MessageType::ERROR, format!("cannot set context: {}", e));
                }

                Ok(DiagnosticBundle {
                    uri: None,
                    diagnostics: vec![],
                })
            }
            Err(err) => {
                let bundle = match err {
                    TypeCheckError::Parse(desc) | TypeCheckError::TypeCheck(desc) => {
                        let uri = desc.file.as_ref().and_then(|path| Url::from_file_path(path).ok());

                        let diagnostic = Diagnostic {
                            range: desc.text_range.map(to_lsp_range).unwrap_or_else(dummy_range),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: desc.message,
                            source: Some("psy-lsp".into()),
                            ..Default::default()
                        };

                        eprintln!("typecheck_lsp failed: uri = {:?}, message = {}", uri, diagnostic.message);
                        DiagnosticBundle {
                            uri,
                            diagnostics: vec![diagnostic],
                        }
                    }
                    TypeCheckError::Cycle(msg) | TypeCheckError::StoragePreprocess(msg) => {
                        let uri = self.root_uri();
                        let diagnostic = Diagnostic {
                            range: dummy_range(),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: msg,
                            source: Some("psy-lsp".into()),
                            ..Default::default()
                        };
                        DiagnosticBundle {
                            uri,
                            diagnostics: vec![diagnostic],
                        }
                    }
                };
                Ok(bundle)
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for QLspSimple {
    async fn initialize(&self, params: InitializeParams) -> TResult<InitializeResult> {
        let root_uri = params.root_uri.ok_or_else(|| {
            let msg = "Missing root_uri in InitializeParams".to_string();
            eprintln!("{msg}");
            TError::invalid_params(msg)
        })?;

        let root_path = root_uri.to_file_path().map_err(|_| {
            let msg = format!("Failed to convert root_uri {:?} to file path", root_uri);
            eprintln!("{msg}");
            TError::invalid_params(msg)
        })?;

        let _ = self.init_and_publish_diagnostics(&root_path).await;

        Ok(InitializeResult {
            capabilities: tower_lsp::lsp_types::ServerCapabilities {
                hover_provider: Some(tower_lsp::lsp_types::HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::INCREMENTAL), // Or FULL
                    will_save: Some(false),
                    will_save_wait_until: Some(false),
                    save: Some(TextDocumentSyncSaveOptions::Supported(true).into()),
                })),
                ..Default::default()
            },
            ..Default::default()
        })
    }
    async fn initialized(&self, _: InitializedParams) {
        dbg!("initialized!");
    }

    async fn shutdown(&self) -> TResult<()> {
        dbg!("shutdown!");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        dbg!(format!("did_open: {:?}", params));
        let uri = &params.text_document.uri;

        self.maybe_publish_cached_diagnostics(uri).await;
    }
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // dbg!(format!("did_change: {:?}", params));
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        dbg!(format!("did_save: {:?}", params));
        let uri = &params.text_document.uri;

        let path = match try_uri_to_path(uri) {
            Some(p) => p,
            None => {
                eprintln!("{:?} to_file_path error", uri);
                return;
            }
        };

        // Check if the file is already in the file_resolver
        if self.is_ready() {
            let ctx_guard = self.get_ctx_read();
            if ctx_guard.program.file_resolver.resolve_id(&path).is_none() {
                eprintln!("{:?} not found in file_resolver", uri);
                return;
            }
        }

        let root_path = self.get_root_path();
        dbg!("did_save prepare init: {:?}", &root_path);
        let _ = self.init_and_publish_diagnostics(&root_path).await;
        dbg!("did_save init Success");

        dbg!(format!("Saved file: {}", uri));
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        dbg!(format!("did_close: {:?}", params));
    }

    async fn goto_definition(&self, params: GotoDefinitionParams) -> TResult<Option<GotoDefinitionResponse>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = &params.text_document_position_params.text_document.uri;

        let path = uri.to_file_path().map_err(|_| QLspError::UriToPathError(uri.to_string()))?;

        let file_id = self.resolve_file_id(&path)?;

        let position = Position {
            file_id,
            line: params.text_document_position_params.position.line as usize,
            column: params.text_document_position_params.position.character as usize,
        };
        dbg!(format!("goto position: {:?}", position));

        let ctx = self.get_ctx_read();

        let location = match ctx.position_to_location(position) {
            Some(loc) => loc,
            None => {
                dbg!("goto definition: failed to map position to location");
                return Ok(None);
            }
        };
        dbg!(format!("goto: source loc {:?}", location));

        let target_location = match ctx.goto_definition(location) {
            Some(target) => target,
            None => {
                dbg!("goto definition: target not found");
                return Ok(None);
            }
        };
        dbg!(format!("goto: target location {:?}", target_location));

        let source_text = ctx
            .program
            .file_resolver
            .resolve_content(&location.file_id)
            .ok_or_else(|| QLspError::Internal(format!("Cannot resolve file content for file_id: {:?}", location.file_id)))?;

        let range = span_to_range(&target_location, &source_text);

        let target_path = ctx
            .program
            .file_resolver
            .resolve_path(&target_location.file_id)
            .ok_or_else(|| QLspError::Internal("Cannot resolve path for target file_id".into()))?;

        let target_uri = if target_path == *path {
            uri.clone()
        } else {
            Url::from_file_path(target_path).map_err(|_| QLspError::InvalidUri("Invalid target file path".to_string()))?
        };
        Ok(Some(GotoDefinitionResponse::Scalar(Location { uri: target_uri, range })))
    }
    async fn references(&self, params: ReferenceParams) -> TResult<Option<Vec<Location>>> {
        if !self.is_ready() {
            return Ok(None);
        }
        let uri = &params.text_document_position.text_document.uri;

        let path = uri.to_file_path().map_err(|_| QLspError::UriToPathError(uri.to_string()))?;

        let file_id = self.resolve_file_id(&path)?;
        let position = Position {
            file_id,
            line: params.text_document_position.position.line as usize,
            column: params.text_document_position.position.character as usize,
        };

        let ctx = self.get_ctx_read();

        let location = ctx
            .position_to_location(position)
            .ok_or_else(|| QLspError::Internal("Cannot find location for position".to_string()))?;

        let locations = match ctx.find_all_references(location, true, false) {
            Some(locs) => locs
                .into_iter()
                .filter(|r| *r != location) //filter out self-reference
                .collect::<Vec<_>>(),
            None => return Ok(None),
        };

        let resolved_locations = locations
            .iter()
            .filter_map(|loc| {
                ctx.program.file_resolver.resolve_content(&loc.file_id).map(|source| {
                    let range = span_to_range(loc, &source);
                    Location { uri: uri.clone(), range }
                })
            })
            .collect::<Vec<_>>();
        Ok(Some(resolved_locations))
    }

    async fn hover(&self, params: HoverParams) -> TResult<Option<Hover>> {
        if !self.is_ready() {
            return Ok(None);
        }
        let uri = &params.text_document_position_params.text_document.uri;
        let path = uri.to_file_path().map_err(|_| QLspError::UriToPathError(uri.to_string()))?;
        let file_id = self.resolve_file_id(&path)?;

        let position = Position {
            file_id,
            line: params.text_document_position_params.position.line as usize,
            column: params.text_document_position_params.position.character as usize,
        };

        let ctx = self.get_ctx_read();
        let location = ctx
            .position_to_location(position)
            .ok_or_else(|| QLspError::Internal("Failed to convert position to location".into()))?;

        let hover_text = ctx.hover(location);

        let hover = hover_text.map(|text| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("**Type**: `{}`", text),
            }),
            range: None,
        });

        Ok(hover)
    }

    async fn completion(&self, params: CompletionParams) -> TResult<Option<CompletionResponse>> {
        dbg!(&params);
        Ok(None)
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> TResult<Option<Vec<TextEdit>>> {
        if !self.is_ready() {
            return Ok(None);
        }
        let DocumentFormattingParams { text_document, .. } = params;

        let uri = &text_document.uri;
        let path = uri.to_file_path().map_err(|_| QLspError::UriToPathError(uri.to_string()))?;
        let mut ctx = self.get_ctx_write();

        let formatted = ctx
            .format_file(&path)
            .map_err(|e| QLspError::Internal(format!("format_file failed: {}", e)))?;

        let text_edit = TextEdit {
            range: Range {
                start: tower_lsp::lsp_types::Position { line: 0, character: 0 },
                end: tower_lsp::lsp_types::Position {
                    //todo!: replace with the actual end position
                    line: 100000,
                    character: 1000,
                },
            },
            new_text: formatted,
        };

        Ok(Some(vec![text_edit]))
    }
    //rename
    async fn rename(&self, params: tower_lsp::lsp_types::RenameParams) -> TResult<Option<tower_lsp::lsp_types::WorkspaceEdit>> {
        dbg!(&params);
        Ok(None)
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        dbg!("configuration changed!");
    }

    async fn did_change_workspace_folders(&self, _: DidChangeWorkspaceFoldersParams) {
        dbg!("workspace folders changed!");
    }
    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        dbg!("watched files have changed!");
    }
}

pub fn try_uri_to_path(uri: &Url) -> Option<PathBuf> {
    match uri.to_file_path() {
        Ok(p) => Some(p),
        Err(_) => {
            dbg!(format!("{:?} to_file_path error", uri));
            None
        }
    }
}

/// `TextPosition` → LSP `Position`
fn to_lsp_position(pos: TextPosition) -> tower_lsp::lsp_types::Position {
    tower_lsp::lsp_types::Position {
        line: pos.line,
        character: pos.character,
    }
}

/// `TextRange` → LSP `Range`
fn to_lsp_range(range: TextRange) -> Range {
    Range {
        start: to_lsp_position(range.start),
        end: to_lsp_position(range.end),
    }
}
/// Dummy range: typically used for unknown or fallback diagnostic positions.
pub fn dummy_range() -> Range {
    Range {
        start: tower_lsp::lsp_types::Position { line: 0, character: 0 },
        end: tower_lsp::lsp_types::Position { line: 0, character: 1 },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use futures::StreamExt as _;
    use psy_ast::{Program, TextPosition, TextRange};
    use psy_common::Graph;
    use psy_sema::TypeCheckerVisitorContext;
    use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};
    use serial_test::serial;
    use tower_lsp::lsp_types::{
        CompletionParams, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentFormattingParams, FormattingOptions,
        GotoDefinitionParams, HoverContents, HoverParams, InitializeParams, InitializedParams, MarkupKind,
        OneOf, PartialResultParams, ReferenceContext, ReferenceParams, RenameParams, TextDocumentIdentifier,
        TextDocumentItem, TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
        TextDocumentSyncSaveOptions, Url, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    };
    use tower_lsp::{LanguageServer, LspService};

    use super::{dummy_range, to_lsp_position, to_lsp_range, try_uri_to_path, DiagnosticBundle, QLspSimple};

    #[test]
    fn uri_conversion_accepts_file_urls_and_rejects_non_file_urls() {
        let file = Url::from_file_path("/tmp/example.psy").unwrap();
        assert_eq!(try_uri_to_path(&file).unwrap().to_str(), Some("/tmp/example.psy"));
        let https = Url::parse("https://example.com/example.psy").unwrap();
        assert!(try_uri_to_path(&https).is_none());
    }

    #[test]
    fn lsp_position_and_range_conversion_preserve_coordinates() {
        let position = to_lsp_position(TextPosition { line: 3, character: 7 });
        assert_eq!(position.line, 3);
        assert_eq!(position.character, 7);

        let range = to_lsp_range(TextRange {
            start: TextPosition { line: 1, character: 2 },
            end: TextPosition { line: 4, character: 5 },
        });
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 2);
        assert_eq!(range.end.line, 4);
        assert_eq!(range.end.character, 5);
    }

    #[test]
    fn dummy_range_is_a_single_character_at_document_start() {
        let range = dummy_range();
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 0);
        assert_eq!(range.end.character, 1);
    }

    /// Builds a backend without spawning the client-socket drain task. Only
    /// suitable for tests that never trigger client notifications.
    fn quiet_backend() -> (LspService<QLspSimple>, ()) {
        let (service, _socket) = LspService::new(QLspSimple::new);
        (service, ())
    }

    /// Builds a backend whose client socket is continuously drained in the
    /// background, so `publish_diagnostics` / `log_message` calls never block.
    fn drained_backend() -> LspService<QLspSimple> {
        let (service, socket) = LspService::new(QLspSimple::new);
        tokio::spawn(async move {
            let mut socket = socket;
            while socket.next().await.is_some() {}
        });
        service
    }

    /// Writes `main.psy` into a fresh temp dir and returns the canonicalized
    /// dir and file paths (canonicalization matches `format_file`'s behavior).
    fn write_psy_workspace(source: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file = dir.path().join("app.psy");
        std::fs::write(&file, source).expect("write app.psy");
        let file = file.canonicalize().expect("canonicalize main.psy");
        let root = dir.path().canonicalize().expect("canonicalize root");
        (dir, root, file)
    }

    fn entry_graph(file: &PathBuf) -> Graph<PathBuf> {
        let mut graph = Graph::new();
        graph.add_node(file.clone());
        graph
    }

    const VALID_MAIN: &str = "fn main(q: Felt) -> Felt {\n    return q;\n}\n";

    /// Cursor position (0-based line, character) of the first `needle` on the
    /// given line. Sources in these tests are ASCII, so byte == character.
    fn position_at(source: &str, line: u32, needle: char) -> tower_lsp::lsp_types::Position {
        let text_line = source.lines().nth(line as usize).unwrap_or_else(|| panic!("line {line} missing"));
        let character = text_line.find(needle).unwrap_or_else(|| panic!("{needle:?} missing on line {line}")) as u32;
        tower_lsp::lsp_types::Position { line, character }
    }

    fn text_document(uri: &Url) -> TextDocumentIdentifier {
        TextDocumentIdentifier { uri: uri.clone() }
    }

    fn position_params(uri: &Url, position: tower_lsp::lsp_types::Position) -> TextDocumentPositionParams {
        TextDocumentPositionParams { text_document: text_document(uri), position }
    }

    #[test]
    fn state_helpers_manage_root_path_graph_and_diagnostics_caches() {
        let (service, _) = quiet_backend();
        let server = service.inner();

        assert_eq!(server.get_root_path(), PathBuf::new());
        assert!(server.root_uri().is_none());
        assert!(server.resolve_file_id(&PathBuf::from("/nowhere/main.psy")).is_err());

        let root = PathBuf::from("/tmp/psy-lsp-state-test");
        server.set_root_path(&root).expect("set root path");
        assert_eq!(server.get_root_path(), root);
        assert!(server.root_uri().is_some());

        server.set_ctx(TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(Program::new())).expect("set ctx");
        assert!(!server.is_ready());

        let entry = PathBuf::from("/tmp/psy-lsp-state-test/main.psy");
        server.set_crate_path_graph_cache(entry.clone(), entry_graph(&entry));
        let cached = server.get_cached_crate_path_graph(&entry).expect("cached graph");
        assert!(cached.contains_node(&entry));
        assert!(server.get_cached_crate_path_graph(&PathBuf::from("/tmp/other.psy")).is_none());

        server.remove_crate_path_graph_cache(&entry);
        assert!(server.get_cached_crate_path_graph(&entry).is_none());
        server.set_crate_path_graph_cache(entry.clone(), entry_graph(&entry));
        server.clear_all_crate_path_graph_cache();
        assert!(server.get_cached_crate_path_graph(&entry).is_none());

        let bundle = DiagnosticBundle {
            uri: Some(Url::from_file_path(&entry).unwrap()),
            diagnostics: vec![Diagnostic {
                range: dummy_range(),
                severity: Some(DiagnosticSeverity::ERROR),
                message: "boom".to_string(),
                source: Some("psy-lsp".to_string()),
                ..Default::default()
            }],
        };
        assert!(server.get_last_diagnostics().is_none());
        server.set_last_diagnostics(bundle);
        assert!(server.get_last_diagnostics().expect("cached bundle").diagnostics.len() == 1);
        server.clear_last_diagnostics();
        assert!(server.get_last_diagnostics().is_none());
    }

    #[tokio::test]
    async fn lifecycle_notifications_and_stub_handlers_are_noops() {
        let service = drained_backend();
        let server = service.inner();
        let uri = Url::from_file_path("/tmp/psy-lsp-lifecycle/example.psy").unwrap();

        server.initialized(InitializedParams {}).await;
        server.did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri: uri.clone(), version: 1 },
            content_changes: vec![],
        })
        .await;
        server.did_close(DidCloseTextDocumentParams { text_document: text_document(&uri) }).await;
        server.did_change_configuration(tower_lsp::lsp_types::DidChangeConfigurationParams {
            settings: serde_json::json!({}),
        })
        .await;
        server.did_change_workspace_folders(Default::default()).await;
        server.did_change_watched_files(tower_lsp::lsp_types::DidChangeWatchedFilesParams { changes: vec![] }).await;

        // A non-file URI cannot be saved, so `did_save` bails out early.
        let https = Url::parse("https://example.com/example.psy").unwrap();
        server.did_save(DidSaveTextDocumentParams { text_document: text_document(&https), text: None }).await;

        // Without an opened document there is nothing to re-publish.
        server.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "psy".to_string(),
                version: 1,
                text: String::new(),
            },
        })
        .await;

        // Completion and rename are not implemented yet.
        let completion = server
            .completion(CompletionParams {
                text_document_position: position_params(&uri, tower_lsp::lsp_types::Position { line: 0, character: 0 }),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
                context: None,
            })
            .await;
        assert!(completion.expect("completion").is_none());

        let rename = server
            .rename(RenameParams {
                text_document_position: position_params(&uri, tower_lsp::lsp_types::Position { line: 0, character: 0 }),
                new_name: "renamed".to_string(),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await;
        assert!(rename.expect("rename").is_none());

        // Before the first successful typecheck every language feature degrades to `None`.
        assert!(!server.is_ready());
        assert!(server.goto_definition(GotoDefinitionParams {
            text_document_position_params: position_params(&uri, tower_lsp::lsp_types::Position { line: 0, character: 0 }),
            work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            partial_result_params: PartialResultParams { partial_result_token: None },
        })
        .await
        .expect("goto not ready")
        .is_none());
        assert!(server
            .hover(HoverParams {
                text_document_position_params: position_params(&uri, tower_lsp::lsp_types::Position { line: 0, character: 0 }),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await
            .expect("hover not ready")
            .is_none());
        assert!(server
            .references(ReferenceParams {
                text_document_position: position_params(&uri, tower_lsp::lsp_types::Position { line: 0, character: 0 }),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
                context: ReferenceContext { include_declaration: true },
            })
            .await
            .expect("references not ready")
            .is_none());
        assert!(server
            .formatting(DocumentFormattingParams {
                text_document: text_document(&uri),
                options: FormattingOptions::default(),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await
            .expect("formatting not ready")
            .is_none());

        server.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn did_open_republishes_cached_diagnostics_for_the_same_document_only() {
        let service = drained_backend();
        let server = service.inner();
        let uri = Url::from_file_path("/tmp/psy-lsp-open/a.psy").unwrap();
        let other_uri = Url::from_file_path("/tmp/psy-lsp-open/b.psy").unwrap();

        server.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "psy".to_string(),
                version: 1,
                text: String::new(),
            },
        })
        .await;
        assert!(server.get_last_diagnostics().is_none());

        // A cached bundle without a URI never re-publishes.
        server.set_last_diagnostics(DiagnosticBundle { uri: None, diagnostics: vec![] });
        server.maybe_publish_cached_diagnostics(&uri).await;

        server.set_last_diagnostics(DiagnosticBundle {
            uri: Some(uri.clone()),
            diagnostics: vec![Diagnostic {
                range: dummy_range(),
                severity: Some(DiagnosticSeverity::ERROR),
                message: "stale".to_string(),
                source: Some("psy-lsp".to_string()),
                ..Default::default()
            }],
        });

        // Opening a different document must not touch the cached bundle.
        server.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: other_uri,
                language_id: "psy".to_string(),
                version: 1,
                text: String::new(),
            },
        })
        .await;
        assert_eq!(server.get_last_diagnostics().expect("still cached").uri, Some(uri.clone()));

        // Re-opening the cached document re-publishes and keeps the cache.
        server.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "psy".to_string(),
                version: 2,
                text: String::new(),
            },
        })
        .await;
        assert_eq!(server.get_last_diagnostics().expect("still cached after open").uri, Some(uri));

        // Clearing drops the cache entirely.
        server.clear_cached_diagnostics().await;
        assert!(server.get_last_diagnostics().is_none());
    }

    #[test]
    #[serial]
    fn collect_diagnostics_sync_bundles_parse_and_type_errors() {
        let (service, _) = quiet_backend();
        let server = service.inner();
        let (dir, root, file) = write_psy_workspace("fn main( {\n}\n");
        server.set_crate_path_graph_cache(root.clone(), entry_graph(&file));

        let bundle = server.collect_diagnostics_sync(&root).expect("parse error bundle");
        let uri = Url::from_file_path(&file).unwrap();
        assert_eq!(bundle.uri, Some(uri.clone()));
        assert_eq!(bundle.diagnostics.len(), 1);
        let diagnostic = &bundle.diagnostics[0];
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostic.source.as_deref(), Some("psy-lsp"));
        assert!(!diagnostic.message.is_empty());
        // The failed typecheck must not have produced a usable context.
        assert!(!server.is_ready());

        // A semantically invalid (but parseable) file yields a type-check diagnostic.
        std::fs::write(&file, "fn main() -> Felt {\n    let x: Felt = 1;\n    x = 2;\n    return x;\n}\n").unwrap();
        let bundle = server.collect_diagnostics_sync(&root).expect("type error bundle");
        assert_eq!(bundle.uri, Some(uri));
        assert_eq!(bundle.diagnostics.len(), 1);
        assert!(bundle.diagnostics[0].message.contains('x'), "unexpected message: {}", bundle.diagnostics[0].message);
        assert!(!server.is_ready());

        // Without a cached crate graph the manifest lookup must fail.
        let orphan_dir = tempfile::tempdir().unwrap();
        let orphan_root = orphan_dir.path().canonicalize().unwrap();
        assert!(server.collect_diagnostics_sync(&orphan_root).is_err());

        drop(dir);
    }

    #[tokio::test]
    #[serial]
    async fn initialize_typechecks_the_workspace_and_drives_the_diagnostics_lifecycle() {
        let service = drained_backend();
        let server = service.inner();
        let (dir, root, file) = write_psy_workspace(VALID_MAIN);
        let uri = Url::from_file_path(&file).unwrap();
        server.set_crate_path_graph_cache(root.clone(), entry_graph(&file));

        // A missing or remote root URI is rejected before any work happens.
        let missing = server.initialize(InitializeParams::default()).await;
        assert!(missing.is_err());
        let remote = server
            .initialize(InitializeParams {
                root_uri: Some(Url::parse("https://example.com/").unwrap()),
                ..Default::default()
            })
            .await;
        assert!(remote.is_err());

        let result = server
            .initialize(InitializeParams {
                root_uri: Some(Url::from_file_path(&root).unwrap()),
                ..Default::default()
            })
            .await
            .expect("initialize succeeds");
        assert!(server.is_ready());
        assert_eq!(server.get_root_path(), root);

        let capabilities = result.capabilities;
        assert_eq!(capabilities.hover_provider, Some(tower_lsp::lsp_types::HoverProviderCapability::Simple(true)));
        assert_eq!(capabilities.definition_provider, Some(OneOf::Left(true)));
        assert_eq!(capabilities.references_provider, Some(OneOf::Left(true)));
        assert_eq!(capabilities.document_formatting_provider, Some(OneOf::Left(true)));
        match capabilities.text_document_sync {
            Some(TextDocumentSyncCapability::Options(options)) => {
                assert_eq!(options.open_close, Some(true));
                assert_eq!(options.change, Some(TextDocumentSyncKind::INCREMENTAL));
                assert_eq!(options.save, Some(TextDocumentSyncSaveOptions::Supported(true)));
            }
            other => panic!("unexpected text document sync capability: {other:?}"),
        }

        // A parse error publishes a diagnostic and remembers it.
        std::fs::write(&file, "fn main( {\n}\n").unwrap();
        server.init_and_publish_diagnostics(&root).await.expect("publish parse error");
        let bundle = server.get_last_diagnostics().expect("cached parse-error bundle");
        assert_eq!(bundle.uri, Some(uri.clone()));
        assert_eq!(bundle.diagnostics.len(), 1);

        // A failing recompile (no manifest) clears the previous diagnostics and logs to the client.
        let orphan_dir = tempfile::tempdir().unwrap();
        let orphan_root = orphan_dir.path().canonicalize().unwrap();
        server.init_and_publish_diagnostics(&orphan_root).await.expect("failure is reported, not propagated");
        assert!(server.get_last_diagnostics().is_none());

        // A successful recompile yields an empty bundle without a URI.
        std::fs::write(&file, VALID_MAIN).unwrap();
        server.init_and_publish_diagnostics(&root).await.expect("publish valid workspace");
        assert!(server.get_last_diagnostics().is_none());
        assert!(server.is_ready());

        drop(dir);
    }

    #[tokio::test]
    #[serial]
    async fn did_save_recompiles_only_known_documents() {
        let service = drained_backend();
        let server = service.inner();
        let (dir, root, file) = write_psy_workspace(VALID_MAIN);
        server.set_crate_path_graph_cache(root.clone(), entry_graph(&file));
        server.collect_diagnostics_sync(&root).expect("initial diagnostics");
        assert!(server.is_ready());

        // Non-file URIs bail out immediately.
        let https = Url::parse("https://example.com/main.psy").unwrap();
        server.did_save(DidSaveTextDocumentParams { text_document: text_document(&https), text: None }).await;

        // A file the workspace never resolved is skipped.
        let unknown = Url::from_file_path(root.join("other.psy")).unwrap();
        server.did_save(DidSaveTextDocumentParams { text_document: text_document(&unknown), text: None }).await;
        assert!(server.get_last_diagnostics().is_none());

        // Saving a known document recompiles the workspace.
        let uri = Url::from_file_path(&file).unwrap();
        server.did_save(DidSaveTextDocumentParams { text_document: text_document(&uri), text: None }).await;
        assert!(server.is_ready());

        drop(dir);
    }

    #[tokio::test]
    #[serial]
    async fn goto_hover_references_and_formatting_answer_real_positions() {
        let service = drained_backend();
        let server = service.inner();
        let (dir, root, file) = write_psy_workspace(VALID_MAIN);
        let uri = Url::from_file_path(&file).unwrap();
        server.set_crate_path_graph_cache(root.clone(), entry_graph(&file));
        server.collect_diagnostics_sync(&root).expect("diagnostics");
        assert!(server.is_ready());

        let usage = position_at(VALID_MAIN, 1, 'q');

        // Non-file URIs and unregistered files are hard errors.
        let https = Url::parse("https://example.com/main.psy").unwrap();
        assert!(server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: position_params(&https, usage),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
            })
            .await
            .is_err());
        assert!(server
            .hover(HoverParams {
                text_document_position_params: position_params(&https, usage),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await
            .is_err());
        assert!(server
            .references(ReferenceParams {
                text_document_position: position_params(&https, usage),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
                context: ReferenceContext { include_declaration: true },
            })
            .await
            .is_err());
        let unknown = Url::from_file_path(root.join("missing.psy")).unwrap();
        assert!(server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: position_params(&unknown, usage),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
            })
            .await
            .is_err());
        assert!(server
            .formatting(DocumentFormattingParams {
                text_document: text_document(&unknown),
                options: FormattingOptions::default(),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await
            .is_err());

        // Positions that are not identifiers resolve to nothing.
        let whitespace = tower_lsp::lsp_types::Position { line: 2, character: 0 };
        assert!(server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: position_params(&uri, whitespace),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
            })
            .await
            .expect("goto whitespace")
            .is_none());
        assert!(server
            .hover(HoverParams {
                text_document_position_params: position_params(&uri, whitespace),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await
            .expect("hover whitespace")
            .is_none());
        assert!(server
            .references(ReferenceParams {
                text_document_position: position_params(&uri, whitespace),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
                context: ReferenceContext { include_declaration: true },
            })
            .await
            .expect("references whitespace")
            .is_none());

        // Jumping from the usage of `q` lands on the parameter declaration.
        let response = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: position_params(&uri, usage),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
            })
            .await
            .expect("goto definition")
            .expect("definition found");
        match response {
            tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(location) => {
                assert_eq!(location.uri, uri);
                assert_eq!(location.range.start, position_at(VALID_MAIN, 0, 'q'));
            }
            other => panic!("unexpected goto definition response: {other:?}"),
        }

        // Hovering the usage reports the variable and its type.
        let hover = server
            .hover(HoverParams {
                text_document_position_params: position_params(&uri, usage),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await
            .expect("hover")
            .expect("hover text");
        match hover.contents {
            HoverContents::Markup(markup) => {
                assert_eq!(markup.kind, MarkupKind::Markdown);
                assert!(markup.value.contains('q'), "unexpected hover text: {}", markup.value);
            }
            other => panic!("unexpected hover contents: {other:?}"),
        }

        // References exclude the queried position itself but keep the declaration.
        let references = server
            .references(ReferenceParams {
                text_document_position: position_params(&uri, usage),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
                partial_result_params: PartialResultParams { partial_result_token: None },
                context: ReferenceContext { include_declaration: true },
            })
            .await
            .expect("references")
            .expect("reference list");
        assert!(!references.is_empty());
        assert!(references.iter().all(|location| location.uri == uri));
        assert!(references
            .iter()
            .any(|location| location.range.start == position_at(VALID_MAIN, 0, 'q')));

        // Formatting replaces the whole document.
        let edits = server
            .formatting(DocumentFormattingParams {
                text_document: text_document(&uri),
                options: FormattingOptions::default(),
                work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
            })
            .await
            .expect("formatting")
            .expect("format edits");
        assert_eq!(edits.len(), 1);
        assert!(edits[0].new_text.contains("fn main"), "unexpected format output: {}", edits[0].new_text);
        assert_eq!(edits[0].range.start, tower_lsp::lsp_types::Position { line: 0, character: 0 });

        drop(dir);
    }
}
