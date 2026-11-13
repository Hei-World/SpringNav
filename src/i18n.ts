import * as vscode from 'vscode';

// Runtime UI strings. VS Code picks the language automatically via
// `vscode.env.language` (e.g. "zh-cn", "en", "en-us"). Command titles, menu
// items and configuration descriptions come from package.nls*.json instead.

type Messages = {
  indexing: string;
  indexingTooltip: string;
  ready: (files: number) => string;
  tooltip: (r: {
    projects: number;
    modules: number;
    files: number;
    methods: number;
    edges: number;
  }) => string;
  tooltipHint: string;
  error: string;
  errorTooltip: string;
  indexFailed: (err: string) => string;
  indexed: (r: {
    files: number;
    projects: number;
    modules: number;
    methods: number;
    edges: number;
    skipped: number;
    failed: number;
  }) => string;
  indexDone: (r: {
    projects: number;
    modules: number;
    files: number;
  }) => string;
  rebuilt: (stats: string) => string;
  reindexed: (name: string, files: number, modules: number) => string;
  reindexedLog: (root: string) => string;
  reindexFailed: (err: string) => string;
  updateFailed: (err: string) => string;
  notJavaProject: string;
  activation: string;
  notFound: (label: string) => string;
  peekPlaceholder: (n: number) => string;
  searchTitle: string;
  searchPlaceholder: string;
  labelImplementation: string;
  labelReference: string;
  skippedAndFailed: (skipped: number, failed: number) => string;
  indexingWait: string;
  contextMenuGroup: string;
};

const en: Messages = {
  indexing: '$(sync~spin) SpringNav: indexing…',
  indexingTooltip: 'SpringNav is building the index',
  ready: (files) => `$(database) SpringNav: ${files} files`,
  tooltip: (r) =>
    `**SpringNav index**\n\n- Projects: ${r.projects}\n- Modules: ${r.modules}\n- Files: ${r.files}\n- Methods: ${r.methods}\n- Edges: ${r.edges}\n\nClick to rebuild the index`,
  tooltipHint: 'Click to rebuild the index',
  error: '$(error) SpringNav: index failed',
  errorTooltip: 'SpringNav index failed, click to retry',
  indexFailed: (err) => `SpringNav: index failed - ${err}`,
  indexed: (r) => {
    const base = `SpringNav: indexed ${r.files} files, ${r.projects} projects, ${r.modules} modules, ${r.methods} methods, ${r.edges} edges`;
    return r.skipped || r.failed
      ? `${base}, ${r.skipped} skipped (>1MB), ${r.failed} failed to parse`
      : base;
  },
  indexDone: (r) =>
    `SpringNav: index ready (${r.projects} projects / ${r.modules} modules / ${r.files} files)`,
  rebuilt: (stats) => `SpringNav: rebuilt — ${stats}`,
  reindexed: (name, files, modules) =>
    `SpringNav: rebuilt ${name} (index now ${files} files / ${modules} modules)`,
  reindexedLog: (root) => `SpringNav: reindexed root ${root}`,
  reindexFailed: (err) => `SpringNav: reindex root failed: ${err}`,
  updateFailed: (err) => `SpringNav: update failed: ${err}`,
  notJavaProject:
    'SpringNav: this folder is not a Java project (no pom.xml / build.gradle)',
  activation: 'SpringNav: activation, indexing started',
  notFound: (label) => `SpringNav: no matching ${label} found`,
  peekPlaceholder: (n) => `SpringNav: ${n} matching locations`,
  searchTitle: 'SpringNav: Search by URL path',
  searchPlaceholder: 'Type a path fragment to search, e.g. /system/user/list',
  labelImplementation: 'implementation',
  labelReference: 'reference',
  skippedAndFailed: (skipped, failed) =>
    `SpringNav: ${skipped} large files skipped, ${failed} files failed to parse`,
  indexingWait: 'SpringNav: index is still being built, please try again shortly',
  contextMenuGroup: 'springnav@1',
};

const zh: Messages = {
  indexing: '$(sync~spin) SpringNav: 索引中…',
  indexingTooltip: 'SpringNav 正在构建索引',
  ready: (files) => `$(database) SpringNav: ${files} 文件`,
  tooltip: (r) =>
    `**SpringNav 索引**\n\n- 项目：${r.projects}\n- 模块：${r.modules}\n- 文件：${r.files}\n- 方法：${r.methods}\n- 边：${r.edges}\n\n点击重新构建索引`,
  tooltipHint: '点击重新构建索引',
  error: '$(error) SpringNav: 索引失败',
  errorTooltip: 'SpringNav 索引失败，点击重试',
  indexFailed: (err) => `SpringNav: 索引失败 - ${err}`,
  indexed: (r) => {
    const base = `SpringNav: 已索引 ${r.files} 文件、${r.projects} 项目、${r.modules} 模块、${r.methods} 方法、${r.edges} 边`;
    return r.skipped || r.failed
      ? `${base}，跳过 ${r.skipped} 个大文件（>1MB），${r.failed} 个文件解析失败`
      : base;
  },
  indexDone: (r) =>
    `SpringNav: 索引完成（${r.projects} 项目 / ${r.modules} 模块 / ${r.files} 文件）`,
  rebuilt: (stats) => `SpringNav: 已重建 — ${stats}`,
  reindexed: (name, files, modules) =>
    `SpringNav: 已重建 ${name} 的索引（索引共 ${files} 文件 / ${modules} 模块）`,
  reindexedLog: (root) => `SpringNav: 已重建项目 ${root}`,
  reindexFailed: (err) => `SpringNav: 项目索引重建失败：${err}`,
  updateFailed: (err) => `SpringNav: 增量更新失败：${err}`,
  notJavaProject:
    'SpringNav: 该目录不是 Java 项目（未找到 pom.xml / build.gradle）',
  activation: 'SpringNav: 已激活，开始构建索引',
  notFound: (label) => `SpringNav: 未找到匹配的${label}`,
  peekPlaceholder: (n) => `SpringNav: 找到 ${n} 个匹配位置`,
  searchTitle: 'SpringNav: 按接口路径搜索',
  searchPlaceholder: '输入路径片段开始检索，例如 /system/user/list',
  labelImplementation: '实现',
  labelReference: '引用',
  skippedAndFailed: (skipped, failed) =>
    `SpringNav: 跳过 ${skipped} 个大文件，${failed} 个文件解析失败`,
  indexingWait: 'SpringNav: 索引仍在构建中，请稍后重试',
  contextMenuGroup: 'springnav@1',
};

/** Pick messages by VS Code display language. */
export function messages(): Messages {
  const lang = (vscode.env.language || 'en').toLowerCase();
  return lang.startsWith('zh') ? zh : en;
}

/** BCP-47-ish language tag for nls-independent use. */
export function language(): string {
  return vscode.env.language || 'en';
}
