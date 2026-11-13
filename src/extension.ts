import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { messages, language } from './i18n';

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
  skipped: number;
  failed: number;
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
  reindexRoot(root: string): BuildResult;
  reloadConfig(): BuildResult;
  getStatus(): BuildResult;
  projectRoots(): string[];
  isReady(): boolean;
  indexStats(): string;
  uriForPath(path: string): string;
}

// eslint-disable-next-line @typescript-eslint/no-var-requires
// The native module is loaded LAZILY inside activate(), guarded by a
// child-process preflight: dlopen of a corrupt/quarantined/wrong-arch binary
// can HARD-ABORT the process (uncatchable in JS). Loading it inside the
// extension host would kill the host on every activation -> restart loop.
// The preflight dies instead; the host survives and reports the error.
let native: SpringNavNative;
let nativeLoadError: string | undefined;

function nativeProxy(reason: string): SpringNavNative {
  return new Proxy({} as SpringNavNative, {
    get() {
      throw new Error(`SpringNav native module unavailable: ${reason}`);
    },
  });
}

/// Try loading the native addon in a throwaway Node child process.
/// dyld aborts (quarantine / bad signature / wrong arch) kill the CHILD,
/// never the extension host.
function preflightNative(extensionPath: string): Promise<{ ok: boolean; error?: string }> {
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const { spawn } = require('child_process') as typeof import('child_process');
  return new Promise((resolve) => {
    const script =
      `try{require(${JSON.stringify(require('path').join(extensionPath, 'native'))});` +
      `process.exit(0)}catch(e){console.error(String(e && e.message || e));process.exit(1)}`;
    let child;
    try {
      child = spawn(process.execPath, ['-e', script], {
        env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' },
        timeout: 20000,
      });
    } catch (e) {
      resolve({ ok: false, error: `preflight spawn failed: ${String(e)}` });
      return;
    }
    let err = '';
    child.stderr?.on('data', (d) => {
      err += String(d);
    });
    child.on('close', (code, signal) => {
      if (code === 0) {
        resolve({ ok: true });
      } else {
        resolve({
          ok: false,
          error:
            `preflight load failed (exit=${code}${signal ? ` signal=${signal}` : ''}) ` +
            err.split('\n').slice(-4).join(' | ').slice(0, 500),
        });
      }
    });
    child.on('error', (e) => {
      resolve({ ok: false, error: `preflight spawn error: ${String(e)}` });
    });
  });
}

interface PathItem extends vscode.QuickPickItem {
  target: vscode.Location;
}

const JAVA_XML = [{ language: 'java' }, { language: 'xml' }];

let statusBar: vscode.StatusBarItem | undefined;
let outputChannel: vscode.OutputChannel | undefined;
let buildSeq = 0;
let indexReady = false;
let waitHintShown = false;

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

/** Nearest project root (pom / build.gradle) for a folder clicked in Explorer. */
function findReindexRoot(folder: string): string {
  const roots = workspaceRoots();
  const root = roots.find(
    (r) => folder === r || folder.startsWith(r + path.sep)
  );
  if (!root) {
    return folder;
  }
  let cur = folder;
  while (true) {
    for (const marker of ['pom.xml', 'build.gradle', 'build.gradle.kts']) {
      if (fs.existsSync(path.join(cur, marker))) {
        return cur;
      }
    }
    if (cur === root) {
      break;
    }
    const parent = path.dirname(cur);
    if (parent === cur || !parent.startsWith(root)) {
      break;
    }
    cur = parent;
  }
  return root;
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
  const t = messages();
  statusBar.text = t.indexing;
  statusBar.tooltip = t.indexingTooltip;
  statusBar.show();
}

function setReady(r: BuildResult): void {
  if (!statusBar) {
    return;
  }
  const t = messages();
  statusBar.text = t.ready(r.files);
  const tip = new vscode.MarkdownString(t.tooltip(r));
  statusBar.tooltip = tip;
  statusBar.show();
  // Expose project roots so the Explorer context menu only shows on project
  // folders (P2 filter). Include both fsPath and URI forms because different
  // VS Code builds populate `resourcePath` differently.
  const roots = native.projectRoots();
  const folderKeys = roots.flatMap((r) => [r, vscode.Uri.file(r).toString()]);
  void vscode.commands.executeCommand(
    'setContext',
    'springNav.projectFolders',
    folderKeys
  );
  if (r.skipped > 0 || r.failed > 0) {
    outputChannel?.appendLine(t.skippedAndFailed(r.skipped, r.failed));
  }
}

function setError(): void {
  if (!statusBar) {
    return;
  }
  const t = messages();
  statusBar.text = t.error;
  statusBar.tooltip = t.errorTooltip;
  statusBar.show();
}

/** Full rebuild across all workspace folders. */
async function runIndex(notify: boolean): Promise<void> {
  const roots = workspaceRoots();
  if (roots.length === 0) {
    return;
  }
  const seq = ++buildSeq;
  indexReady = false;
  waitHintShown = false;
  setIndexing();
  try {
    const r = await native.buildIndex(roots);
    if (seq !== buildSeq) {
      return; // superseded by a newer rebuild
    }
    indexReady = true;
    setReady(r);
    const t = messages();
    outputChannel?.appendLine(t.indexed(r));
    if (notify) {
      vscode.window.setStatusBarMessage(t.indexDone(r), 4000);
    }
  } catch (err) {
    if (seq !== buildSeq) {
      return;
    }
    setError();
    vscode.window.showErrorMessage(messages().indexFailed(String(err)));
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
    vscode.window.showInformationMessage(messages().notFound(label));
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
    placeHolder: messages().peekPlaceholder(locs.length),
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
  outputChannel = vscode.window.createOutputChannel('SpringNav');
  const version = context.extension.packageJSON.version as string;

  // 1) Child-process preflight: never dlopen a possibly-fatal binary in-host.
  const pf = await preflightNative(context.extensionPath);
  if (!pf.ok) {
    nativeLoadError = pf.error;
    native = nativeProxy(nativeLoadError ?? 'preflight failed');
    const msg = messages();
    const diag =
      `platform=${process.platform} arch=${process.arch} version=${version} | ` +
      nativeLoadError +
      ' | fix: `npm run build:native` on this machine, or `xattr -cr <extension dir>` if quarantined';
    outputChannel.appendLine(`SpringNav: native load FAILED — ${diag}`);
    statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBar.text = msg.error;
    statusBar.tooltip = diag;
    statusBar.command = 'springNav.reindex';
    statusBar.show();
    context.subscriptions.push(statusBar);
    void vscode.window.showErrorMessage(msg.error + ' — ' + diag);
    return;
  }

  // 2) Preflight passed; load in-host (JS-level errors still caught).
  try {
    native = require('../native');
  } catch (err) {
    nativeLoadError = String(err instanceof Error ? err.message : err);
    native = nativeProxy(nativeLoadError);
    outputChannel.appendLine(`SpringNav: native load FAILED — ${nativeLoadError}`);
    void vscode.window.showErrorMessage(`SpringNav: ${nativeLoadError}`);
    return;
  }

  const roots = workspaceRoots();
  if (roots.length === 0) {
    return;
  }
  // F-01: activationEvents already used workspaceContains; re-validate here.
  if (!roots.some((r) => native.isJavaProject(r))) {
    return;
  }
  context.subscriptions.push(outputChannel);

  statusBar = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    100
  );
  statusBar.command = 'springNav.reindex';
  context.subscriptions.push(statusBar);

  // F-02: lazy, asynchronous build. Activation does NOT wait for it.
  void runIndex(false);
  // Log the exact build being loaded — stale-version residue is a common
  // "still broken" misdiagnosis after reinstalling.
  outputChannel?.appendLine(
    `SpringNav: v${version ?? '?'} (${process.platform}-${process.arch}) ${messages().activation}`
  );
  outputChannel?.appendLine(`SpringNav: language = ${language()}`);

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
        // Empty-window hint: while indexing, explain instead of a silent
        // "No definition found". Shown at most once per indexing period.
        if (locs.length === 0 && !indexReady && !waitHintShown) {
          waitHintShown = true;
          vscode.window.showInformationMessage(messages().indexingWait);
        }
        // Returning Location[] lets VS Code show a peek list for multi-hits (M-01).
        return toLocations(locs);
      },
    })
  );

  // Native Go to References (Shift+F12 / Find All References): definition ->
  // all references, falling back to definition when nothing references it.
  context.subscriptions.push(
    vscode.languages.registerReferenceProvider(JAVA_XML, {
      provideReferences(document: vscode.TextDocument, position: vscode.Position) {
        const snippet = document.lineAt(position.line).text;
        const locs = native.resolveDirectional(
          document.uri.toString(),
          position.line,
          position.character,
          snippet,
          'up',
          fallbackOnEmpty()
        );
        return toLocations(locs);
      },
    })
  );

  // Right-click menu: Go to Java Reference (up) / Go to Java Definition (down).
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.goToImplementation', async () => {
      await reveal(resolveAt('down'), messages().labelImplementation);
    }),
    vscode.commands.registerCommand('springNav.goToReference', async () => {
      await reveal(resolveAt('up'), messages().labelReference);
    })
  );

  // Manual full rebuild (command + status bar click).
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.reindex', async () => {
      await runIndex(true);
      outputChannel?.appendLine(messages().rebuilt(native.indexStats()));
    })
  );

  // Explorer right-click on a project folder: rebuild only that project.
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.reindexFolder', async (uri?: vscode.Uri) => {
      if (!uri) {
        return;
      }
      const target = findReindexRoot(uri.fsPath);
      if (!native.isJavaProject(target)) {
        vscode.window.showWarningMessage(messages().notJavaProject);
        return;
      }
      setIndexing();
      try {
        const r = native.reindexRoot(target);
        setReady(r);
        vscode.window.setStatusBarMessage(
          messages().reindexed(path.basename(target), r.files, r.modules),
          3000
        );
        outputChannel?.appendLine(messages().reindexedLog(target));
      } catch (err) {
        setError();
        outputChannel?.appendLine(messages().reindexFailed(String(err)));
      }
    })
  );

  // S-01: real-time interface path search.
  context.subscriptions.push(
    vscode.commands.registerCommand('springNav.searchByPath', async () => {
      const qp = vscode.window.createQuickPick<PathItem>();
      qp.title = messages().searchTitle;
      qp.placeholder = messages().searchPlaceholder;
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
          indexReady = true;
          setReady(r);
        } catch (err) {
          setError();
          outputChannel?.appendLine(messages().updateFailed(String(err)));
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
