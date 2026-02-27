import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import { execFile } from "child_process";
import * as https from "https";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";

let client: LanguageClient | undefined;
let log: vscode.OutputChannel;

const GITHUB_REPO = "jnjaeschke/webspec-index";

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

function platformTarget(): string | null {
  const platform = process.platform;
  const arch = process.arch;
  if (platform === "linux" && arch === "x64")
    return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64")
    return "aarch64-unknown-linux-gnu";
  if (platform === "darwin" && arch === "x64")
    return "x86_64-apple-darwin";
  if (platform === "darwin" && arch === "arm64")
    return "aarch64-apple-darwin";
  if (platform === "win32" && arch === "x64")
    return "x86_64-pc-windows-msvc";
  return null;
}

function binaryName(): string {
  return process.platform === "win32" ? "webspec-index.exe" : "webspec-index";
}

// ---------------------------------------------------------------------------
// Managed binary paths
// ---------------------------------------------------------------------------

function managedBinaryDir(context: vscode.ExtensionContext): string {
  return path.join(context.globalStorageUri.fsPath, "bin");
}

function managedBinaryPath(context: vscode.ExtensionContext): string {
  return path.join(managedBinaryDir(context), binaryName());
}

function versionFilePath(context: vscode.ExtensionContext): string {
  return path.join(managedBinaryDir(context), "version.json");
}

function installedVersion(context: vscode.ExtensionContext): string | null {
  try {
    const data = JSON.parse(fs.readFileSync(versionFilePath(context), "utf-8"));
    return data.version ?? null;
  } catch {
    return null;
  }
}

function extensionVersion(context: vscode.ExtensionContext): string {
  return context.extension.packageJSON.version;
}

// ---------------------------------------------------------------------------
// HTTP download helper (follows redirects)
// ---------------------------------------------------------------------------

function download(url: string, dest: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    const request = (u: string) => {
      https
        .get(u, { headers: { "User-Agent": "webspec-lens" } }, (res) => {
          if (
            res.statusCode &&
            res.statusCode >= 300 &&
            res.statusCode < 400 &&
            res.headers.location
          ) {
            file.close();
            request(res.headers.location);
            return;
          }
          if (res.statusCode !== 200) {
            file.close();
            fs.unlinkSync(dest);
            reject(new Error(`Download failed: HTTP ${res.statusCode} for ${u}`));
            return;
          }
          res.pipe(file);
          file.on("finish", () => {
            file.close();
            resolve();
          });
        })
        .on("error", (err) => {
          file.close();
          fs.unlinkSync(dest);
          reject(err);
        });
    };
    request(url);
  });
}

// ---------------------------------------------------------------------------
// Binary download + extraction
// ---------------------------------------------------------------------------

async function downloadBinary(
  context: vscode.ExtensionContext,
  version: string
): Promise<string> {
  const target = platformTarget();
  if (!target) {
    throw new Error(
      `Unsupported platform: ${process.platform}-${process.arch}`
    );
  }

  const ext = process.platform === "win32" ? "zip" : "tar.gz";
  const assetName = `webspec-index-${target}.${ext}`;
  const url = `https://github.com/${GITHUB_REPO}/releases/download/v${version}/${assetName}`;

  const binDir = managedBinaryDir(context);
  const binPath = managedBinaryPath(context);

  return vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: `Installing webspec-index v${version}...`,
      cancellable: false,
    },
    async () => {
      const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "webspec-"));
      const archive = path.join(tmpDir, assetName);

      try {
        log.appendLine(`Downloading ${url}`);
        await download(url, archive);

        fs.mkdirSync(binDir, { recursive: true });

        log.appendLine(`Extracting to ${binDir}`);
        await new Promise<void>((resolve, reject) => {
          execFile("tar", ["xf", archive, "-C", binDir], (err) => {
            if (err) reject(new Error(`Extraction failed: ${err.message}`));
            else resolve();
          });
        });

        if (process.platform !== "win32") {
          fs.chmodSync(binPath, 0o755);
        }

        fs.writeFileSync(
          versionFilePath(context),
          JSON.stringify({ version }) + "\n"
        );

        log.appendLine(`Installed webspec-index v${version} to ${binPath}`);
        return binPath;
      } finally {
        fs.rmSync(tmpDir, { recursive: true, force: true });
      }
    }
  );
}

// ---------------------------------------------------------------------------
// Server discovery
// ---------------------------------------------------------------------------

async function commandExists(cmd: string): Promise<boolean> {
  return new Promise((resolve) => {
    const which = process.platform === "win32" ? "where" : "which";
    execFile(which, [cmd], (error) => {
      resolve(!error);
    });
  });
}

async function findServerCommand(
  context: vscode.ExtensionContext
): Promise<string[]> {
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

  // 3. Managed binary (previously downloaded)
  const binPath = managedBinaryPath(context);
  const extVer = extensionVersion(context);
  const curVer = installedVersion(context);

  if (fs.existsSync(binPath)) {
    if (curVer === extVer) {
      log.appendLine(`Using managed binary v${curVer} at ${binPath}`);
      return [binPath, "lsp"];
    }
    // Extension was updated — re-download matching binary
    log.appendLine(
      `Managed binary v${curVer} differs from extension v${extVer}, updating...`
    );
    try {
      await downloadBinary(context, extVer);
      return [binPath, "lsp"];
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      log.appendLine(`Update failed, using existing binary: ${msg}`);
      return [binPath, "lsp"];
    }
  }

  // 4. Offer to download
  if (!platformTarget()) {
    throw new Error(
      `Could not find webspec-index on PATH. Unsupported platform for auto-install: ${process.platform}-${process.arch}. Install manually: cargo install webspec-index`
    );
  }

  const choice = await vscode.window.showInformationMessage(
    "webspec-index not found. Download it?",
    "Install",
    "Cancel"
  );

  if (choice !== "Install") {
    throw new Error(
      "Could not find webspec-index. Install with: cargo binstall webspec-index"
    );
  }

  await downloadBinary(context, extVer);
  return [binPath, "lsp"];
}

// ---------------------------------------------------------------------------
// Extension lifecycle
// ---------------------------------------------------------------------------

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
    command = await findServerCommand(context);
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
