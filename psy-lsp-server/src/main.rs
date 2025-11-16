use psy_lsp_server::simple::QLspSimple;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    eprintln!("Starting Psy LSP server...");
    let (service, socket) = LspService::build(QLspSimple::new).finish();
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    Server::new(stdin, stdout, socket).serve(service).await;
}
