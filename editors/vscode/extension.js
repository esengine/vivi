// Vivi Language Server client — starts vivi-lsp as a subprocess.
// This is the only JS in the entire Vivi project, required by VS Code's extension API.
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const vscode = require("vscode");

let client;

function activate(context) {
    const lspPath = vscode.workspace.getConfiguration("vivi").get("lspPath", "vivi-lsp");

    const serverOptions = {
        command: lspPath,
        transport: TransportKind.stdio,
    };

    const clientOptions = {
        documentSelector: [{ scheme: "file", language: "vivi" }],
    };

    client = new LanguageClient("vivi-lsp", "Vivi Language Server", serverOptions, clientOptions);
    client.start();
}

function deactivate() {
    if (client) return client.stop();
}

module.exports = { activate, deactivate };
