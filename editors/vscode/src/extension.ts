import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import { execFile } from "child_process";

let client: LanguageClient | undefined;
let log: vscode.OutputChannel;

async function commandExists(cmd: string): Promise<boolean> {
  return new Promise((resolve) => {
    execFile("which", [cmd], (error) => {
      resolve(!error);
    });
  });
}

async function findServerCommand(): Promise<string[]> {
  // 1. User setting
  const configured = vscode.workspace
    .getConfiguration("webspecLens")
    .get<string[]>("serverCommand");
  log.appendLine(`serverCommand setting: ${JSON.stringify(configured)}`);
  if (configured && configured.length > 0) return configured;

  // 2. webspec-index on PATH
  if (await commandExists("webspec-index")) {
    log.appendLine("Found webspec-index on PATH");
    return ["webspec-index", "lsp"];
  }

  throw new Error(
    "Could not find webspec-index. Install with: cargo binstall webspec-index"
  );
}

export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  log = vscode.window.createOutputChannel("webspec-lens");
  log.appendLine("webspec-lens activating...");

  const config = vscode.workspace.getConfiguration("webspecLens");
  if (!config.get<boolean>("enabled", true)) {
    log.appendLine("Extension disabled via setting");
    return;
  }

  // Register coverage detail command
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "webspecLens.showCoverage",
      (anchor: string, total: number, missing: string[]) => {
        const impl = total - missing.length;
        let msg = `${anchor}: ${impl}/${total} steps implemented`;
        if (missing.length > 0) {
          msg += `\nMissing steps: ${missing.join(", ")}`;
        }
        vscode.window.showInformationMessage(msg);
      }
    )
  );

  let command: string[];
  try {
    command = await findServerCommand();
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    vscode.window.showWarningMessage(`webspec-lens: ${msg}`);
    return;
  }

  log.appendLine(`Starting server: ${JSON.stringify(command)}`);

  const serverOptions: ServerOptions = {
    command: command[0],
    args: command.slice(1),
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", pattern: "**/*" }],
    outputChannel: log,
    initializationOptions: {
      fuzzyThreshold: config.get<number>("fuzzyThreshold", 0.85),
    },
  };

  client = new LanguageClient(
    "webspecLens",
    "webspec-lens",
    serverOptions,
    clientOptions
  );

  try {
    await client.start();
    log.appendLine("Server started successfully");
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    log.appendLine(`Server failed to start: ${msg}`);
    vscode.window.showWarningMessage(
      `webspec-lens: Failed to start LSP server: ${msg}`
    );
  }
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}
