import * as vscode from 'vscode';
import * as path from 'path';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: vscode.ExtensionContext) {
    const serverExecutable = path.join(
        //waring: this path is hardcoded, it should be changed to a more dynamic path
        context.extensionPath, '..', '..',  '..', 'target', 'release', 'psy-lsp-server'
    );

    const serverOptions: ServerOptions = {
        run: { command: serverExecutable, transport: TransportKind.stdio },
        debug: { command: serverExecutable, transport: TransportKind.stdio }
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: 'psy' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.psy')
        }
    };

    client = new LanguageClient(
        'psyLanguageServer',
        'Psy Language Server',
        serverOptions,
        clientOptions
    );

    client.start();
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
