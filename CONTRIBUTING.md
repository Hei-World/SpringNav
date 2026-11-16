# 参与贡献

SpringNav 以 MIT 协议开源，欢迎使用、反馈和讨论。但本仓库**不接受外部 Pull Request**，请先阅读下面的反馈方式。

## 如何反馈

| 类型 | 渠道 |
| --- | --- |
| 可复现的 Bug（跳错、不跳、崩溃、索引异常） | [Bug 报告](https://github.com/Hei-World/SpringNav/issues/new?template=bug_report.yml) |
| 新功能 / 跳转场景建议 | [功能建议](https://github.com/Hei-World/SpringNav/issues/new?template=feature_request.yml) |
| 安装、配置、使用方法等问题 | [Discussions](https://github.com/Hei-World/SpringNav/discussions) |
| 安全漏洞 | **不要开公开 Issue**，按 [SECURITY.md](SECURITY.md) 私密报告 |

提交 Bug 时请尽量附上：SpringNav 版本、操作系统与架构、VS Code 版本、触发代码片段（接口 / 实现类 / 调用点）、期望与实际跳转位置、`SpringNav` 输出通道日志、状态栏悬停的索引统计。

## 代码贡献政策

本仓库已关闭 Pull Request 功能：

- 任何人都无法向 `Hei-World/SpringNav` 发起合并请求；
- Fork 仍然可以，你可以自由修改用于自己使用，但请遵守 MIT 许可证；
- 如果你有修复思路，欢迎在对应 Issue 中说明：场景、最小复现、原因分析、建议改法，必要时附代码片段或 patch 文本。维护者评估后会自行实现并发布。

这样做的原因：项目对性能、内存和解析准确率有硬性约束，跳转规则需要整体一致，由单人统一维护可以避免规则冲突和回归成本。

## 维护者开发环境

| 工具 | 版本 |
| --- | --- |
| Node.js | 18+（CI 使用 20） |
| Rust | stable（`rustup` 安装即可） |
| VS Code | 1.74+ |

```bash
npm ci                     # 必须使用 lockfile
npm run build              # tsc + cargo build --release，产出 native/spring-nav.<platform>.node
npm run test:native        # 合成工程自检（85 项）
node test/harness.js /path/to/RuoYi-Vue   # 真实项目端到端（32 项）
cargo test --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings
```

`test/harness.js` 默认读取 `/tmp/RuoYi-Vue`，也可以传任意 SpringBoot 工程根目录：

```bash
git clone --depth 1 https://github.com/yangzongzhuan/RuoYi-Vue.git /tmp/RuoYi-Vue
```

## 代码约定

- **解析层**（`rust/src/parser.rs`）：逐行扫描 + 状态机 + 正则；字符串掩码必须保持**字节等长**，列号一律经 `col()` 转换，避免中文多字节边界 panic。
- **索引层**（`rust/src/index.rs`）：新增跳转关系必须产出标准 `k0` / `k1` / `k2` 边，走 `edge_contrib`，保证增量失效自动覆盖；类型解析统一用 `resolve_type_files_ctx`（imports → 同包 → 全项目唯一简单名，歧义返回空）。
- **查询层**（`rust/src/resolve.rs`）：只做内存查表；兜底逻辑不得重新解析源码。
- **TS 壳**（`src/extension.ts`）：零业务逻辑，只做参数转换、Provider 注册与防御加载；所有 napi 入口必须保持 `catch_unwind` 防线。
- 文案改动同时更新 `package.nls.json`、`package.nls.zh-cn.json`、`src/i18n.ts`，保持「定义 / 引用」概念一致。
- 不引入 JVM、Java 语言服务（JDT LS / Red Hat Java）、tree-sitter、WASM 或外部源码下载；不在查询路径上读磁盘或重新解析文件。

## 测试要求

- 修 bug：必须补一条能复现旧行为的断言（合成夹具优先，真实项目场景补到 `test/harness.js`）。
- 加功能：`scripts/smoke.js` 至少覆盖正向跳转、反向引用、歧义不跳三种情况；涉及跨文件边时补「改文件 → 边失效 → 恢复」的增量断言。
- 涉及解析/索引的改动，需要记录：全量索引耗时、查询耗时、增量耗时、100 次重建后的 RSS 与索引计数器，与 [AGENTS.md](AGENTS.md) 的「测试体系」基线对比。

## 分支与发布（维护者）

- `master` 是唯一长期分支，始终保持可构建、可发布；不设 `develop`。
- 本地可以使用短分支（`feat/*`、`fix/*`、`docs/*`、`chore/*`），完成后 squash 到 `master` 并删除。
- `master` 受规则保护：禁止删除、禁止强推。
- 发布流程：
  1. 更新 `package.json` 与 `package-lock.json`（两处 `version`）到新版本；
  2. 同步 `README.md`（用户视角）、`AGENTS.md`（详细上下文）与 `CHANGELOG.md`；
  3. 提交并推送 `master`；
  4. 打 `v<version>` tag 并推送，GitHub Actions 自动测试、构建 6 架构 `.node`、打全平台 VSIX 并发布 Release。

## 许可证

贡献（包括在 Issue 中提供的代码片段）表示你同意以 [MIT](LICENSE) 许可证授权，允许维护者将其并入本项目。
