import * as vscode from 'vscode';

// The TS shell is intentionally logic-free (P-05): it only activates, registers
// providers/commands, passes parameters through and converts Rust locations
// into vscode.Location. All matching happens inside the napi-rs addon.

interface NativeLocation {
  uri: string;
  line: number;
  character: number;
}

interface SpringNavNative {
  isJavaProject(root: string): boolean;
  buildIndex(root: string): Promise<number>;
  resolveDefinition(
    uri: string,
    line: number,
    character: number,
    contextSnippet: string
  ): NativeLocation[];
  searchByPath(path: string): NativeLocation[];
  updateFile(uri: string): void;
  reloadConfig(): void;
  indexStats(): string;
  uriForPath(path: string): string;
}

// eslint-disable-next-line @typescript-eslint/no-var-requires
const native: SpringNavNative = require('../native');

function toLocations(locs: NativeLocation[]): vscode.Location[] {
  return locs.map(
    (l) =>
      new vscode.Location(
        vscode.Uri.parse(l.uri),
        new vscode.Position(l.line, l.character)
      )
  );
}

function workspaceRoot(): string | undefined {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

async function openLocation(loc: vscode.Location): Promise<void> {
  await vscode.window.showTextDocument(loc.uri, { selection: loc.range });
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const root = workspaceRoot();
  if (!root) {
    return;
  }
  // F-01: activationEvents already used workspaceContains; re-validate here.
  if (!native.isJavaProject(root)) {
    return;
  }

  const channel = vscode.window.createOutputChannel('SpringNav');
  context.subscriptions.push(channel);

  // F-02: lazy, asynchronous, off the extension host's main thread. Activation
  // does NOT wait for the build; providers simply return empty until it lands.
  void native
    .buildIndex(root)
    .then((count) => {
      channel.appendLine(`SpringNav: indexed ${count} files — ${native.indexStats()}`);
    })
    .catch((err) => {
      channel.appendLine(`SpringNav: index build failed: ${String(err)}`);
    });

  // DefinitionProvider: pure parameter passing + result conversion.
  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider(
      [{ language: 'java' }, { language: 'xml' }],
      {
        provideDefinition(document: vscode.TextDocument, position: vscode.Position) {
          const snippet = document.lineAt(position.line).text;
          const locs = native.resolveDefinition(
            document.uri.toString(),
            position.line,
            position.character,
            snippet
          );
          // Returning Location[] lets VS Code show a peek list for multi-hits (M-01).
          return toLocations(locs);
        },
      }
    )
  );

  // S-01: search an interface URL path.
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.searchByPath', async () => {
      const input = await vscode.window.showInputBox({
        prompt: '输入接口路径，例如 /v1/getInfo',
        placeHolder: '/system/user/list',
      });
      if (!input) {
        return;
      }
      const locs = toLocations(native.searchByPath(input));
      if (locs.length === 0) {
        vscode.window.showInformationMessage('SpringNav: 未找到匹配的接口路径');
        return;
      }
      if (locs.length === 1) {
        await openLocation(locs[0]);
        return;
      }
      const picks = locs.map((loc) => ({
        label: `${vscode.workspace.asRelativePath(loc.uri)}:${loc.range.start.line + 1}`,
        description: loc.uri.fsPath,
        loc,
      }));
      const picked = await vscode.window.showQuickPick(picks, {
        placeHolder: `SpringNav: 找到 ${locs.length} 个匹配位置`,
      });
      if (picked) {
        await openLocation(picked.loc);
      }
    })
  );

  // F-03: incremental update on save, only the saved file is re-parsed.
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((doc: vscode.TextDocument) => {
      if (doc.languageId === 'java' || doc.uri.fsPath.endsWith('.xml')) {
        native.updateFile(doc.uri.toString());
      }
    })
  );

  // C-04: config hot reload -> local in-memory index rebuild.
  const watcher = vscode.workspace.createFileSystemWatcher(
    '**/.idea/spring-nav.{toml,json}'
  );
  const onConfigChange = () => native.reloadConfig();
  watcher.onDidChange(onConfigChange);
  watcher.onDidCreate(onConfigChange);
  watcher.onDidDelete(onConfigChange);
  context.subscriptions.push(watcher);
}

export function deactivate(): void {
  // Nothing to dispose beyond context.subscriptions.
}
