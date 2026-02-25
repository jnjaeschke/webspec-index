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

async function findPython(): Promise<string | null> {
  if (await commandExists("python3")) return "python3";
  if (await commandExists("python")) return "python";
  return null;
}

async function findServerCommand(): Promise<string[]> {
  // 1. User setting
  const configured = vscode.workspace
    .getConfiguration("specLens")
    .get<string[]>("serverCommand");
  log.appendLine(`serverCommand setting: ${JSON.stringify(configured)}`);
  if (configured && configured.length > 0) return configured;

  // 2. webspec-index on PATH
  if (await commandExists("webspec-index")) {
    log.appendLine("Found webspec-index on PATH");
    return ["webspec-index", "lsp"];
  }

  // 3. uvx
  if (await commandExists("uvx")) {
    log.appendLine("Falling back to uvx");
    return ["uvx", "webspec-index[lsp]", "lsp"];
  }

  // 4. python -m webspec_index lsp
  const python = await findPython();
  if (python) {
    log.appendLine(`Falling back to ${python} -m`);
    return [python, "-m", "webspec_index", "lsp"];
  }

  throw new Error(
    "Could not find webspec-index. Install with: pip install webspec-index[lsp]"
  );
}

export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  log = vscode.window.createOutputChannel("spec-lens");
  log.appendLine("spec-lens activating...");

  const config = vscode.workspace.getConfiguration("specLens");
  if (!config.get<boolean>("enabled", true)) {
    log.appendLine("Extension disabled via setting");
    return;
  }

  // Register coverage detail command
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "specLens.showCoverage",
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
    vscode.window.showWarningMessage(`spec-lens: ${msg}`);
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
    "specLens",
    "spec-lens",
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
      `spec-lens: Failed to start LSP server: ${msg}`
    );
  }
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}
