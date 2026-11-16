# 参与贡献

感谢你愿意改进 SpringNav。这个项目对**性能、内存、资源占用**有硬性要求，提交前请先读完本文。

## 先讨论，再写代码

- 新功能、解析规则调整、任何可能影响索引耗时或内存的改动，请先开 Issue 说明场景与方案，确认口径后再动手。
- 只修文档、注释、测试的 PR 可以直接提交。
- 不接受为了覆盖率牺牲准确率的改动：同名类/多实现有歧义时应当「不跳」，而不是猜一个。

## 硬性边界

以下方案不会被合并：

- 引入 JVM、Java 语言服务（JDT LS / Red Hat Java）、tree-sitter、WASM 或外部源码下载；
- 在查询路径上读磁盘、重新解析文件，或引入阻塞式全量扫描；
- 绕过 `compute_file_edges` 另建旁路缓存，导致增量失效不完整；
- 破坏「索引只存边与紧凑元数据，不存源码/语法树」的设计。

## 环境要求

| 工具 | 版本 |
| --- | --- |
| Node.js | 18+（CI 使用 20） |
| Rust | stable（`rustup` 安装即可） |
| VS Code | 1.74+ |

## 构建与测试

```bash
npm ci                     # 必须使用 lockfile
npm run build              # tsc + cargo build --release，产出 native/spring-nav.<platform>.node
npm run test:native        # 合成工程自检（78 项）
node test/harness.js /path/to/RuoYi-Vue   # 真实项目端到端（32 项）
cargo test --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings
```

`test/harness.js` 默认读取 `/tmp/RuoYi-Vue`，也可以传任意 SpringBoot 工程根目录；仓库不带测试工程，请自行 `git clone --depth 1 https://github.com/yangzongzhuan/RuoYi-Vue.git /tmp/RuoYi-Vue`。

## 代码约定

- **解析层**（`rust/src/parser.rs`）：逐行扫描 + 状态机 + 正则；字符串掩码必须保持**字节等长**，列号一律经 `col()` 转换，避免中文多字节边界 panic。
- **索引层**（`rust/src/index.rs`）：新增跳转关系必须产出标准 `k0` / `k1` / `k2` 边，走 `edge_contrib`，保证增量失效自动覆盖；类型解析统一用 `resolve_type_files_ctx`（imports → 同包 → 全项目唯一简单名，歧义返回空）。
- **查询层**（`rust/src/resolve.rs`）：只做内存查表；兜底逻辑不得重新解析源码。
- **TS 壳**（`src/extension.ts`）：零业务逻辑，只做参数转换、Provider 注册与防御加载；所有 napi 入口必须保持 `catch_unwind` 防线。
- 文案改动同时更新 `package.nls.json`、`package.nls.zh-cn.json`、`src/i18n.ts`，保持「定义 / 引用」概念一致。

## 测试要求

- 修 bug：必须补一条能复现旧行为的断言（合成夹具优先，真实项目场景补到 `test/harness.js`）。
- 加功能：`scripts/smoke.js` 至少覆盖正向跳转、反向引用、歧义不跳三种情况；涉及跨文件边时补「改文件 → 边失效 → 恢复」的增量断言。
- 涉及解析/索引的改动，请在 PR 中附上本地基线：全量索引耗时、查询耗时、增量耗时、100 次重建后的 RSS 与索引计数器（与 README「测试与性能基线」对比）。

## 提交与 PR

- 一个 PR 只做一件事；提交信息用 `type(scope): 摘要`，例如 `fix(parser): 支持带空格的泛型返回类型`。
- PR 描述里写清：改了什么、为什么、对性能/内存的影响、跑了哪些测试。
- 合并前 CI 必须全绿（clippy、cargo test、smoke、RuoYi harness）。
- 版本号由维护者在发布时统一递增，PR 不需要改 `package.json` 的 `version`。

## 发布流程（维护者）

1. 更新 `package.json` 与 `package-lock.json`（两处 `version`）到新版本，同步 README 与 `CHANGELOG.md`；
2. 推送 `master`；
3. 打 `v<version>` tag 并推送，GitHub Actions 自动测试、构建 6 架构 `.node`、打全平台 VSIX 并发布 Release。

## 许可证

贡献即表示你同意以 [MIT](LICENSE) 许可证授权你的改动（inbound = outbound）。
