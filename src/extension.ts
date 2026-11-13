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

interface BuildResult {
  files: number;
  projects: number;
  modules: number;
  methods: number;
  edges: number;
}

interface SpringNavNative {
  isJavaProject(root: string): boolean;
  buildIndex(roots: string[]): Promise<BuildResult>;
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
  updateFiles(uris: string[]): BuildResult;
  updateFile(uri: string): void;
  reloadConfig(): BuildResult;
  getStatus(): BuildResult;
  indexStats(): string;
  uriForPath(path: string): string;
}

// eslint-disable-next-line @typescript-eslint/no-var-requires
const native: SpringNavNative = require('../native');

interface PathItem extends vscode.QuickPickItem {
  target: vscode.Location;
}

const JAVA_XML = [{ language: 'java' }, { language: 'xml' }];

let statusBar: vscode.StatusBarItem | undefined;
let outputChannel: vscode.OutputChannel | undefined;
let buildSeq = 0;

function toLocations(locs: NativeLocation[]): vscode.Location[] {
  return locs.map(
    (l) =>
      new vscode.Location(
        vscode.Uri.parse(l.uri),
        new vscode.Position(l.line, l.character)
      )
  );
}

/** All workspace folders (multi-root supported). */
function workspaceRoots(): string[] {
  return (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath);
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

function setIndexing(): void {
  if (!statusBar) {
    return;
  }
  statusBar.text = '$(sync~spin) SpringNav: 索引中…';
  statusBar.tooltip = 'SpringNav 正在构建索引';
  statusBar.show();
}

function setReady(r: BuildResult): void {
  if (!statusBar) {
    return;
  }
  statusBar.text = `$(database) SpringNav: ${r.files} 文件`;
  statusBar.tooltip = new vscode.MarkdownString(
    `**SpringNav 索引**\n\n- 项目：${r.projects}\n- 模块：${r.modules}\n- 文件：${r.files}\n- 方法：${r.methods}\n- 边：${r.edges}\n\n点击重新构建索引`
  );
  statusBar.show();
}

function setError(): void {
  if (!statusBar) {
    return;
  }
  statusBar.text = '$(error) SpringNav: 索引失败';
  statusBar.tooltip = 'SpringNav 索引失败，点击重试';
  statusBar.show();
}

/** Full rebuild across all workspace folders. */
async function runIndex(notify: boolean): Promise<void> {
  const roots = workspaceRoots();
  if (roots.length === 0) {
    return;
  }
  const seq = ++buildSeq;
  setIndexing();
  try {
    const r = await native.buildIndex(roots);
    if (seq !== buildSeq) {
      return; // superseded by a newer rebuild
    }
    setReady(r);
    outputChannel?.appendLine(
      `SpringNav: indexed ${r.files} files, ${r.projects} projects, ${r.modules} modules, ${r.methods} methods, ${r.edges} edges`
    );
    if (notify) {
      vscode.window.setStatusBarMessage(
        `SpringNav: 索引完成（${r.projects} 项目 / ${r.modules} 模块 / ${r.files} 文件）`,
        4000
      );
    }
  } catch (err) {
    if (seq !== buildSeq) {
      return;
    }
    setError();
    vscode.window.showErrorMessage(`SpringNav: 索引失败 - ${String(err)}`);
  }
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
    // Rust already applied the fuzzy filter; stop VS Code from re-filtering by
    // label (which would hide "/system/user/list" for a query like "user list").
    alwaysShow: true,
    target: new vscode.Location(
      vscode.Uri.parse(h.uri),
      new vscode.Position(h.line, h.character)
    ),
  }));
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const roots = workspaceRoots();
  if (roots.length === 0) {
    return;
  }
  // F-01: activationEvents already used workspaceContains; re-validate here.
  if (!roots.some((r) => native.isJavaProject(r))) {
    return;
  }

  outputChannel = vscode.window.createOutputChannel('SpringNav');
  context.subscriptions.push(outputChannel);

  statusBar = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    100
  );
  statusBar.command = 'springNav.reindex';
  context.subscriptions.push(statusBar);

  // F-02: lazy, asynchronous build. Activation does NOT wait for it.
  void runIndex(false);
  outputChannel?.appendLine('SpringNav: activation, indexing started');

  // Ctrl/Cmd+Click goes through Go to Definition -> this provider.
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

  // Manual full rebuild (command + status bar click).
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.reindex', async () => {
      await runIndex(true);
      outputChannel?.appendLine(`SpringNav: rebuilt — ${native.indexStats()}`);
    })
  );

  // S-01: real-time interface path search.
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.searchByPath', async () => {
      const qp = vscode.window.createQuickPick<PathItem>();
      qp.title = 'SpringNav: 按接口路径搜索';
      qp.placeholder = '输入路径片段开始检索，例如 /system/user/list';
      qp.matchOnDescription = false;
      qp.matchOnDetail = false;
      qp.onDidChangeValue((value) => {
        qp.items = value.trim() ? toPathItems(native.searchPathHits(value)) : [];
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
      qp.items = [];
      qp.show();
    })
  );

  // F-03: incremental update on save. Debounced so a burst of saves triggers a
  // single rebuild; Rust only re-parses the changed files (no disk re-scan).
  const pending = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  context.subscriptions.push({
    dispose: () => {
      if (timer) {
        clearTimeout(timer);
      }
    },
  });
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((doc: vscode.TextDocument) => {
      if (!(doc.languageId === 'java' || doc.uri.fsPath.endsWith('.xml'))) {
        return;
      }
      pending.add(doc.uri.toString());
      if (timer) {
        clearTimeout(timer);
      }
      timer = setTimeout(() => {
        timer = undefined;
        const uris = [...pending];
        pending.clear();
        if (uris.length === 0) {
          return;
        }
        try {
          setIndexing();
          const r = native.updateFiles(uris);
          setReady(r);
        } catch (err) {
          setError();
          outputChannel?.appendLine(`SpringNav: update failed: ${String(err)}`);
        }
      }, 250);
    })
  );

  // C-04: config hot reload -> in-memory rebuild (no file re-read).
  const watcher = vscode.workspace.createFileSystemWatcher(
    '**/.idea/spring-nav.{toml,json}'
  );
  const onConfigChange = () => {
    try {
      setReady(native.reloadConfig());
    } catch {
      setError();
    }
  };
  watcher.onDidChange(onConfigChange);
  watcher.onDidCreate(onConfigChange);
  watcher.onDidDelete(onConfigChange);
  context.subscriptions.push(watcher);

  // Multi-root: rebuild when folders are added/removed at runtime.
  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      void runIndex(false);
    })
  );
}

export function deactivate(): void {
  statusBar?.dispose();
  statusBar = undefined;
}
