import * as vscode from 'vscode';

// The TS shell is intentionally logic-free (P-05): it only activates, registers
// providers/commands, passes parameters through and converts Rust locations
// into vscode.Location. All matching happens inside the napi-rs addon.

interface NativeLocation {
  uri: string;
  line: number;
  character: number;
}

interface NativePathHit {
  path: string;
  file: string;
  method: string;
  module: string;
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
  resolveDirectional(
    uri: string,
    line: number,
    character: number,
    contextSnippet: string,
    direction: string,
    fallback: boolean
  ): NativeLocation[];
  searchByPath(path: string): NativeLocation[];
  searchPathHits(query: string): NativePathHit[];
  updateFile(uri: string): void;
  reloadConfig(): void;
  indexStats(): string;
  uriForPath(path: string): string;
}

// eslint-disable-next-line @typescript-eslint/no-var-requires
const native: SpringNavNative = require('../native');

interface PathItem extends vscode.QuickPickItem {
  target: vscode.Location;
}

const JAVA_XML = [{ language: 'java' }, { language: 'xml' }];

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

function defaultDirection(): 'down' | 'up' | 'both' {
  const v = vscode.workspace
    .getConfiguration('springNav')
    .get<string>('defaultDirection', 'down');
  return v === 'up' || v === 'both' ? v : 'down';
}

function fallbackOnEmpty(): boolean {
  return vscode.workspace
    .getConfiguration('springNav')
    .get<boolean>('fallbackOnEmpty', true);
}

/** Resolve at the cursor with an explicit direction. */
function resolveAt(direction: 'down' | 'up' | 'both'): vscode.Location[] {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    return [];
  }
  const pos = editor.selection.active;
  const snippet = editor.document.lineAt(pos.line).text;
  const locs = native.resolveDirectional(
    editor.document.uri.toString(),
    pos.line,
    pos.character,
    snippet,
    direction,
    fallbackOnEmpty()
  );
  return toLocations(locs);
}

/** Single result: open directly. Multiple: peek list. None: normal hint. */
async function reveal(locs: vscode.Location[], label: string): Promise<void> {
  if (locs.length === 0) {
    vscode.window.showInformationMessage(`SpringNav: 未找到匹配的${label}`);
    return;
  }
  if (locs.length === 1) {
    await vscode.window.showTextDocument(locs[0].uri, {
      selection: locs[0].range,
    });
    return;
  }
  const editor = vscode.window.activeTextEditor;
  if (editor) {
    try {
      await vscode.commands.executeCommand(
        'editor.action.peekLocations',
        editor.document.uri,
        editor.selection.active,
        locs
      );
      return;
    } catch {
      // fall through to QuickPick
    }
  }
  const picks = locs.map((loc) => ({
    label: `${vscode.workspace.asRelativePath(loc.uri)}:${loc.range.start.line + 1}`,
    loc,
  }));
  const picked = await vscode.window.showQuickPick(picks, {
    placeHolder: `SpringNav: 找到 ${locs.length} 个匹配位置`,
  });
  if (picked) {
    await vscode.window.showTextDocument(picked.loc.uri, {
      selection: picked.loc.range,
    });
  }
}

function toPathItems(hits: NativePathHit[]): PathItem[] {
  return hits.map((h) => ({
    label: h.path,
    description: `((${h.file}) [${h.method}])${h.module ? '  ' + h.module : ''}`,
    target: new vscode.Location(
      vscode.Uri.parse(h.uri),
      new vscode.Position(h.line, h.character)
    ),
  }));
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

  // Ctrl/Cmd+Click goes through Go to Definition -> this provider.
  // Default direction comes from `springNav.defaultDirection` (default: down),
  // with an automatic fallback to the opposite direction.
  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider(JAVA_XML, {
      provideDefinition(document: vscode.TextDocument, position: vscode.Position) {
        const snippet = document.lineAt(position.line).text;
        const locs = native.resolveDirectional(
          document.uri.toString(),
          position.line,
          position.character,
          snippet,
          defaultDirection(),
          fallbackOnEmpty()
        );
        // Returning Location[] lets VS Code show a peek list for multi-hits (M-01).
        return toLocations(locs);
      },
    })
  );

  // Right-click menu: 转到Java实现 (down) / 转到Java引用 (up).
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.goToImplementation', async () => {
      await reveal(resolveAt('down'), '实现');
    }),
    vscode.commands.registerCommand('springNav.goToReference', async () => {
      await reveal(resolveAt('up'), '引用');
    })
  );

  // S-01: real-time interface path search (QuickPick filters as you type).
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.searchByPath', async () => {
      const qp = vscode.window.createQuickPick<PathItem>();
      qp.title = 'SpringNav: 按接口路径搜索';
      qp.placeholder = '输入路径片段实时过滤，例如 /system/user/list';
      qp.matchOnDescription = false;
      qp.matchOnDetail = false;
      qp.onDidChangeValue((value) => {
        qp.items = toPathItems(native.searchPathHits(value));
      });
      qp.onDidAccept(async () => {
        const selected = qp.selectedItems[0];
        if (!selected) {
          return;
        }
        qp.hide();
        await vscode.window.showTextDocument(selected.target.uri, {
          selection: selected.target.range,
        });
      });
      qp.onDidHide(() => qp.dispose());
      qp.items = toPathItems(native.searchPathHits(''));
      qp.show();
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
