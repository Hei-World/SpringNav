# 安全策略

## 支持范围

| 版本 | 安全更新 |
| --- | --- |
| 0.3.x | 支持 |
| < 0.3 | 不支持，请先升级 |

## 报告漏洞

**请不要在公开 Issue 中披露安全问题。**

优先使用 GitHub 的私密报告入口：仓库 **Security → Report a vulnerability**（[直达链接](https://github.com/Hei-World/SpringNav/security/advisories/new)）。如果该入口不可用，请通过维护者 GitHub 主页提供的联系方式私下联系。

报告中请包含：

- 受影响版本与平台（Windows / macOS / Linux，架构）；
- VS Code 版本、是否与 Red Hat Java 共存；
- 复现步骤或最小工程结构；
- 实际影响（崩溃、任意文件读取、越权跳转、索引损坏等）；
- 如可能，附 `SpringNav` 输出通道日志与 `SpringNav: 索引统计` 的结果。

## 处理流程

- 收到报告后 7 天内确认并给出初步判断（个人维护，尽力而为）；
- 确认的漏洞会在修复后发布新版本，并在 Release Notes 中说明影响范围；
- 修复前请勿公开细节；修复发布后可协商披露时间。

## 安全边界与已知设计

- 插件只解析工作区内被索引的 Java/XML 文件，**不执行被索引项目的代码**，不启动 JVM，不访问网络。
- 原生模块通过 `catch_unwind` 封锁 panic，异常信息经 `lastPanic` 上报，不会击穿扩展宿主。
- 大于 1MB 的文件会被跳过；`node_modules`、`target` 等目录不会被扫描。
- 配置只读取各项目根目录下的 `.idea/spring-nav.toml` / `.json`，不读取其它路径。
- 不属于安全问题的范围：VS Code 自身或其它插件的漏洞、第三方 Java 库的问题、需要本机代码执行权限才能触发的场景。

## 依赖安全

- Dependabot 每周检查 npm / Cargo / GitHub Actions 依赖并提交 PR；
- CI 在合并前运行 clippy、单元测试与端到端回归；
- 发布产物由 GitHub Actions 从 tag 构建，构建日志公开可查。
