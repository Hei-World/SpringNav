# SpringNav

[![CI](https://github.com/Hei-World/SpringNav/actions/workflows/build.yml/badge.svg)](https://github.com/Hei-World/SpringNav/actions/workflows/build.yml)
[![Release](https://img.shields.io/github/v/release/Hei-World/SpringNav?display_name=tag)](https://github.com/Hei-World/SpringNav/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![VS Code](https://img.shields.io/badge/VS%20Code-%5E1.74-007ACC)](package.json)

> 当前版本 `0.3.7` ｜ [下载安装包](https://github.com/Hei-World/SpringNav/releases/latest) ｜ MIT License

## 项目说明

SpringNav 是一个面向 VS Code / Qoder 的轻量级 **SpringBoot + MyBatis 代码导航插件**。它只做跨文件跳转：**转到 Java 定义、转到 Java 引用、按接口路径搜索**。

它不启动 Java 语言服务，不做语法高亮、补全、诊断与重构，也不替代 Red Hat Java。目标是解决 SpringBoot 分层项目里「调用点 → 接口 → 实现类 → Mapper → XML」和反向查找引用时反复搜索的问题。

核心约束：

- **性能无感**：全量索引毫秒级，查询微秒级，保存后的增量索引毫秒级；
- **内存与资源占用最小**：不启动 JVM，不建 AST，不保存源码；
- **准确率优先**：同名类、多实现、多重载有歧义时返回空或候选列表，不猜测跳转。

当前仓库只开放阅读与问题反馈，不接受外部 Pull Request；反馈方式见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 架构设计与核心原则

```
VS Code（TypeScript 壳，零逻辑）
  DefinitionProvider / ReferenceProvider / Command / Watcher
        │ 同步调用，只传当前行与位置
        ▼
spring-nav.<platform>.node（Rust / napi-rs）
  scanner → parser → index → resolve
  只存「解析完成的边」与紧凑元数据，不存源码、不存语法树
```

| 原则 | 实现 |
| --- | --- |
| 不引入 JVM / 语言服务 / AST | Rust 手写逐行解析器 + 正则；无 tree-sitter、无 WASM、无外部源码 |
| 查询零 I/O | 建索引时预计算 `k0` 无向、`k1` 有向（`down`/`up`）、`k2` 别名边；查询只做内存查表 |
| 增量而非全量 | 保存时只重解析变更文件；`edge_contrib` / `loc_owner` / `call_cache` 定向失效 |
| 不阻塞编辑器 | 索引用 napi `AsyncTask` 在 libuv 线程池构建；查询走读锁 |
| 不拖死扩展宿主 | `.node` 预加载检查；所有 napi 入口 `catch_unwind`；panic 经 `lastPanic` 上报 |
| 中文源码安全 | 字符串掩码按字节等长替换，列号经 char boundary 兜底，不因多字节字符偏移或崩溃 |

详细数据模型、增量策略与解析规则见 [AGENTS.md](AGENTS.md)。

## 功能使用说明

### 转到 Java 定义

**入口**：光标放在调用点或声明上，`Ctrl` / `Cmd` + 单击，或按 `F12`，或右键「转到 Java 定义」。

| 光标位置 | 跳转到 |
| --- | --- |
| `userService.selectUserList(user)` 的方法名 | `ISysUserService.selectUserList` 接口方法 |
| 接口方法名（再按一次） | `SysUserServiceImpl.selectUserList` 实现类方法；多实现时给候选 |
| 实现类里的 `userMapper.selectUserList(...)` | `SysUserMapper.selectUserList`，有 XML 时继续到 `<select id="selectUserList">` |
| `this.foo()` / 同类裸调用 `foo()` | 本类或父类/祖父类中最近的定义（覆盖优先） |
| `getUserName()` / `setUserName(...)` | 实体字段 `userName`（手写与 Lombok 访问器都支持） |
| `SysUser` 等类型名 | 类声明（参数、局部变量、字段、`new X()`、`extends` / `implements`） |
| `@Qualifier("sysUserServiceImpl")` 字符串 | 对应类声明 |
| `Class.CONST` / `Class.method()` | 常量字段或静态方法定义 |

连续使用同一个入口即可完成 Controller → Service 接口 → 实现类 → Mapper → XML 的链路；某个方向没有结果时默认回退到相反方向一次（`springNav.fallbackOnEmpty`）。

### 转到 Java 引用

**入口**：光标放在方法名、字段名、XML 语句 `id` 或类名上，按 `Shift+F12`，或右键「转到 Java 引用」。

| 光标位置 | 结果 |
| --- | --- |
| Service / Mapper 接口方法声明 | 所有调用点（Controller、定时任务、其它 Service）以及实现类方法 |
| 实现类方法声明 | 接口方法 + 所有调用点 |
| 实体字段 | 所有读取/写入点，以及 get/set 方法 |
| `getUserName()` 等访问器 | 字段声明 + 其它调用方 |
| Mapper 方法 | 对应的 XML 语句（若存在） |
| XML `<select id="...">` | 对应的 Mapper 方法，再跳到实现类中的调用方 |

结果以 VS Code peek 列表展示，按文件路径与行号去重排序，回车打开。

### 搜索接口

**入口**：`Ctrl+Shift+P` → `SpringNav: 按接口路径搜索`（英文界面 `SpringNav: Search by URL path`）。

| 项目 | 说明 |
| --- | --- |
| 输入 | 打开时为空，输入即过滤，无需回车；清空恢复为空 |
| 匹配 | 完整路径、路径片段、多关键词（空格分隔），如 `/system/user/list`、`user list`、`sys user` |
| 每行显示 | 完整 URL、文件名、HTTP 方法、所属模块 |
| 操作 | 回车直接打开对应 Controller 方法 |
| URL 来源 | 类级 `@RequestMapping` + 方法级 mapping 拼接；`@GetMapping("getInfo")` 这类不带前导 `/` 的写法同样拼接；路径变量原样显示，如 `/system/user/{userId}` |
| 方法没写路径 | 只有 `@PostMapping` / `@PutMapping` / `@GetMapping` 而没有路径参数时，直接使用类路径本身（如 `/chat`），不会漏掉 |
| 多请求方式 | 同一路径按 HTTP 方法分别成行：`GET /chat`、`POST /chat`、`PUT /chat`，用行尾 `[GET]` / `[POST]` 区分；`@RequestMapping(method = {GET, POST})` 会列出两条 |
| 类路径尾斜杠 | `/chat/` 自动归一化为 `/chat` |
| 常量路径 | 路径写成常量/表达式（如 `@GetMapping(ApiPaths.USER)`）时无法静态解析，不生成条目，也不会误落到类路径 |

## 使用说明

### 安装

- 从 [Releases](https://github.com/Hei-World/SpringNav/releases/latest) 下载 `spring-nav-<version>.vsix`，在扩展面板右上角 `...` → 「从 VSIX 安装」；
- 或命令行：`code --install-extension spring-nav-0.3.7.vsix`；
- 安装包内含 6 个平台的 `.node`，运行时只加载当前平台的一个，其余仅占磁盘、不占内存；
- 打开含 `pom.xml` / `build.gradle` 的工程后自动异步建索引，状态栏出现 `SpringNav: N 文件` 即就绪。

### 索引与多项目

- 状态栏悬停可看项目/模块/方法/边数量；点击状态栏或执行 `SpringNav: 重新加载索引` 全量重建。
- 资源管理器右键项目/模块 → `SpringNav: 重建此项目索引`，不影响其它项目。
- 多根工作区与多模块项目只扫一次，嵌套根自动去重；模块名显示为相对路径。
- 保存文件只重解析该文件；`git pull` 等外部修改需手动重建。

### 配置

在工程根目录放 `.idea/spring-nav.toml` 或 `.idea/spring-nav.json`，完整示例见 [`.idea/spring-nav.toml`](.idea/spring-nav.toml)，schema 见 [`schemas/spring-nav.schema.json`](schemas/spring-nav.schema.json)。

```toml
[aliases]                 # 优先级最高：字段名/类型名 -> 目标类型
# "userService" = "ISysUserService"

[type-roles]              # 覆盖角色推断
# "SysConfigController" = "controller"

[impl-patterns]           # 找不到 implements 时按类名匹配实现
service = ["*ServiceImpl", "*Impl"]
mapper = ["*Mapper", "*Dao", "*Repository"]
```

优先级：**aliases > type-roles > impl-patterns > 默认命名约定**；配置变更自动热加载并局部重建。

### VS Code 设置

| 设置 | 默认 | 说明 |
| --- | --- | --- |
| `springNav.defaultDirection` | `down` | `Ctrl/Cmd+点击` 采用的方向：`down` 定义、`up` 引用、`both` 两者 |
| `springNav.fallbackOnEmpty` | `true` | 指定方向无结果时是否回退到相反方向一级 |

插件默认只占用 `F12`。需要更多键位可在 Keyboard Shortcuts 自行绑定 `springNav.goToImplementation` / `springNav.goToReference`。

## 构建、打包

前置：Node ≥ 18、Rust stable；Linux 需 gcc/make，macOS 需 Xcode CLI Tools，Windows 需 MSVC Build Tools。

```bash
npm ci                 # 依赖锁定，必须使用 lockfile
npm run build:native   # cargo build --release → native/spring-nav.<platform>.node
npm run compile        # tsc → out/extension.js
npm run build          # 以上两步
npm run package        # 构建 + 打包 spring-nav-<version>.vsix
```

同系统跨架构：

```bash
SN_TARGET=x86_64-apple-darwin        bash scripts/build-native.sh
SN_TARGET=aarch64-pc-windows-msvc    bash scripts/build-native.sh
SN_TARGET=aarch64-unknown-linux-gnu  bash scripts/build-native.sh
```

正式发布由 GitHub Actions 完成：推送 `v*` tag 后自动运行测试、构建 6 架构 `.node`、打成单个全平台 VSIX 并发布 Release。CI 矩阵、产物命名与平台限制见 [AGENTS.md](AGENTS.md)。

## 文档与反馈

| 内容 | 位置 |
| --- | --- |
| 详细功能清单、解析规则、测试基线、迭代流程 | [AGENTS.md](AGENTS.md) |
| 反馈渠道与开发约定 | [CONTRIBUTING.md](CONTRIBUTING.md) |
| 版本变更 | [CHANGELOG.md](CHANGELOG.md) |
| 安全问题（请勿公开 Issue） | [SECURITY.md](SECURITY.md) |

可复现的 Bug 与功能建议请提 [Issue](https://github.com/Hei-World/SpringNav/issues)，使用问题到 [Discussions](https://github.com/Hei-World/SpringNav/discussions)。仓库已关闭 Pull Request，不接受外部代码合并。

## 开源协议

本项目以 [MIT](LICENSE) 协议开源，Copyright (c) 2026 Hei-World。可自由使用、修改、分发与商用，保留版权与许可声明即可。

第三方组件许可证与商标说明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。SpringNav 是独立第三方项目，与 VMware / Broadcom、Oracle、Microsoft 无隶属或背书关系。
