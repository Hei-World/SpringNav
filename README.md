# SpringNav

> **当前版本：`0.3.4`** ｜ 全平台安装包：<https://github.com/Hei-World/SpringNav/releases/tag/v0.3.4> ｜ 许可证：MIT

面向 VS Code 的**轻量级 SpringBoot 代码导航插件**。它不运行 Java 语言服务、不做语法高亮/补全/诊断，只做一件事：建立跨文件跳转索引，让你在 SpringBoot / MyBatis 工程里流畅地「跳过去、跳回来」。

核心约束：**极致性能、最小内存占用、最小资源占用**。TS 壳零逻辑，解析与匹配全部在 Rust 里完成，通过 napi-rs 编译为原生 `.node` 模块。任何新功能都必须先评估对性能、内存、资源占用的影响，用户体验必须无感。

---

## 架构

```
VS Code (TypeScript 壳, 零逻辑)  ──同步调用──>  spring-nav.<platform>.node (Rust / napi-rs)
  activate / DefinitionProvider / ReferenceProvider / Command / Watchers
  仅: 取当前行文本、传参、把 Loc 转成 vscode.Location
      ├─ scanner.rs   枚举文件、相关性过滤、import 闭包二次扫描、大文件跳过
      ├─ parser.rs    逐行扫描 + 状态机 + 正则（等长掩码保护 UTF-8 字节偏移）
      ├─ config.rs    每个项目自己的 .idea/spring-nav.toml
      ├─ model.rs     紧凑元数据结构体
      ├─ index.rs     HashMap + 预计算邻接图 + 增量失效
      ├─ resolve.rs   纯内存查表 / 路径搜索 / 行文本兜底
      └─ lib.rs       #[napi] 导出、AsyncTask、panic 全线封锁
```

关键设计：

- **不启动 JDT LS、不依赖 Red Hat Java、不用 tree-sitter、不用 WASM、不引入 JVM**（性能/内存硬约束）。
- 解析器是 Rust 手写轻量解析器（逐行扫描 + 状态机 + 正则），**不构建完整 AST**；索引**只存解析完成的边**（k0 无向 / k1 有向 down+up / k2 别名）与紧凑元数据，不存源码、不存语法树。RuoYi（210 文件）索引数据结构净大小约 30MB 量级。
- 构建阶段把所有跳转关系预计算成有向边（`down` / `up`）并登记光标 span。查询时只做「`(uri,line)` 找 span → 命中符号 → 查方向表 → 去重排序」，**纯内存查表，无磁盘 I/O、不重新解析文件**，Rust 侧微秒级。
- **增量失效**：`edge_contrib`（每文件产出的边）/ `loc_owner` / `call_cache`（调用点解析缓存，含 targets / from_lombok / field_based）三张表按「自身或边目标文件变脏」定向失效，只重算受影响文件，不扫盘。
- `buildIndex` 用 napi `AsyncTask` 在 libuv 线程池异步构建，**不阻塞扩展激活**；查询走 `RwLock` 读锁。
- **稳定性防线**：`preflightNative` 用 `ELECTRON_RUN_AS_NODE` 子进程预试加载 `.node`，所有 napi 入口（含 buildIndex）用 `catch_unwind` 封锁 panic 穿越 FFI，原生模块异常不会杀死扩展宿主；panic 信息经 `indexStats` 的 `lastPanic` 上报。
- 字符串掩码采用**字节等长替换**（多字节中文换成等长空格），`col()` 带 char boundary floor 兜底，保证中文源码的列号与字节偏移一致。

## 概念：定义与引用

所有跳转锚定在「**引用 ↔ 定义**」上，**不存在声明→声明链**：

| 概念 | 含义 | 典型场景 |
| --- | --- | --- |
| **转到 Java 定义**（`down`） | 引用 → 声明 | 方法调用 → 方法定义；get/set 调用 → 实体字段；Mapper 方法 → XML 语句；Service 接口方法 → 实现类方法；类型名/常量引用 → 声明 |
| **转到 Java 引用**（`up`） | 声明 → 引用方 | 方法定义 → 所有调用点；实体字段 → 所有 get/set；XML 语句 → Mapper 方法；Mapper 方法 → 实现类中的调用方 |

`down` / `up` 是同一组 k1 边的双向视图，天然构成定义/引用互逆。某个方向无匹配时默认**自动回退相反方向一级**（`springNav.fallbackOnEmpty`）；两方向都没有则显示标准 `No definition found`。多结果即邻接表里的多个位置，TS 映射成 `Location[]`，VS Code 以 peek 列表展示。

> 历史版本曾用「向下 / 向上」表述链路逐级跳转，已废弃：控制层调用 Service 方法是「引用→定义」；点击 Service 接口方法声明时，转到定义 = 实现类方法，转到引用 = 调用方，不是层级链。

---

## 支持的功能

### 索引与工程

| 编号 | 功能 |
| --- | --- |
| F-01 | Java 项目检测：工作区存在 `pom.xml` / `build.gradle` / `build.gradle.kts` 才激活 |
| F-02 | 相关性过滤（Controller / Service / ServiceImpl / Mapper / Entity / `*Mapper.xml` 等）+ **import 闭包二次扫描**：工具类、常量类即使不符合后缀规则也会被纳入索引 |
| F-03 | 惰性异步索引构建，不阻塞扩展激活 |
| F-04 | **真增量**：保存时只重新解析变更的 java/xml 文件（250ms 防抖、批量合并）；边与缓存定向失效，不扫盘 |
| F-05 | 状态栏 `SpringNav: N 文件`，悬停显示项目/模块/方法/边数量；点击可全量重建 |
| F-06 | 多根工作区 + 多模块项目：每项目只扫一次，嵌套根自动去重；资源管理器右键文件夹可单独重建某项目索引 |
| F-07 | **项目级配置**：每个子项目自己的 `.idea/spring-nav.toml` 生效，配置变更热加载并局部重建 |
| F-08 | 中英国际化：按 VS Code 语言自动切换命令、提示、菜单文案（`package.nls.json` / `package.nls.zh-cn.json` / `src/i18n.ts`） |
| F-09 | 防御性加载 + panic 全线封锁（见架构）；原生模块缺失/损坏时显示诊断信息而非拖死宿主 |
| F-10 | 大于 1MB 的文件跳过并计数上报，避免解析失控 |

### 核心跳转（定义 ↔ 引用）

| 编号 | 功能 |
| --- | --- |
| J-01 | 方法调用 → 定义（Controller 调 Service、ServiceImpl 调 Mapper、同类/父类方法） |
| J-02 | Service 接口方法 ↔ 实现类方法（多实现返回候选列表） |
| J-03 | Mapper 接口方法 ↔ XML 语句：**精确 `(namespace, id)` 匹配**（模糊前缀匹配已移除，避免 `selectUserList` 误配 `selectUserIdByPhonenumber`） |
| J-04 | 实体字段 ↔ get/set：手写 accessor、`getXxx` / `isXxx` / `setXxx`、Lombok 合成访问器 |
| J-05 | 转到引用：声明 → 所有引用方（已对接 VS Code 原生 `ReferenceProvider` / `Shift+F12`） |
| J-06 | 常量 / 静态变量 / 枚举引用 → 字段声明（`Class.CONST`、静态导入成员、裸 `ALL_CAPS` 常量） |

### 调用点语义（P1 / P2）

| 编号 | 功能 |
| --- | --- |
| P1-1 | `this.xxx()` 与同类裸调用 `xxx()`：沿继承链查找，就近覆盖优先 |
| P1-2 | 静态方法调用：大写接收者按 import / 同包 / 全项目唯一简单名解析 |
| P1-3 | 参数 / 局部变量接收者：`body.getLoopNote()` → `@Data` 实体字段 |
| P1-4 | 点击接收者变量名 → 字段声明（k2 别名） |
| P1-5 | 手写 get/set → 实体字段（任意角色，不限于 Entity） |
| P2-1 | `Class::method` 方法引用 → 方法定义或 Lombok 字段 |
| P2-2 | lambda 参数、for-each 变量的类型解析 |
| P2-3 | 静态导入常量、枚举常量引用、裸常量引用 |
| P2-4 | **点击类型名 → 类声明**（参数 / 局部变量 / 字段声明、`new X()`、`extends` / `implements`） |

### 精度与消歧

| 编号 | 功能 |
| --- | --- |
| A2 | **上下文感知类型解析**（`resolve_type_files_ctx`）：imports → 同包 → 全项目唯一简单名；**歧义返回空**（宁可不跳，不跳错）。统一覆盖调用点、常量引用、方法引用、静态调用、类型名点击、继承上溯与行文本兜底 |
| A3 | **多层继承**：`find_methods` 内建 BFS 逐层上溯（visited 环检测、深度上限 8），最近的匹配胜出，覆盖方法优先 |
| A6a | **注解跳转**：`@Autowired` / `@Resource` / `@Inject` 的接口字段 → **唯一实现类**（多实现/无实现不连边）；`@Qualifier("bean")` / `@Resource(name="bean")` 字符串 → 类声明（精确简单名，或 Spring 默认首字母大写 bean 名，且要求唯一匹配） |

> A2 是「减法」：同名类场景下原本混入的错误候选会变成不跳，这是精度修正而非功能退化。

### 路径、结果与配置

| 编号 | 功能 |
| --- | --- |
| S-01 | `springNav.searchByPath` 实时搜索列表：输入即过滤，显示完整路径 + 文件名 + HTTP 方法 + 模块名，支持多关键词 |
| S-02 | 类级 + 方法级 `@RequestMapping` / `@GetMapping` 等注解路径拼接为完整 URL |
| M-01 / M-02 | 多结果 peek 列表；结果去重 + 按文件路径与行号排序 |
| C-01 / C-02 | 内置命名约定；`.idea/spring-nav.toml`（或 `.json`）支持类型角色覆盖 / 实现类命名模式 / 显式别名 |
| C-03 / C-04 | 优先级 **显式别名 > 角色覆盖 > 默认命名约定**；配置热加载 |

---

## 用法

打开一个 SpringBoot 工程（含 `pom.xml` 等），插件自动激活并异步建索引。输出通道 `SpringNav` 打印索引统计。

### 入口

| 入口 | 行为 |
| --- | --- |
| `Ctrl` / `Cmd` + 鼠标左键点击（原生 Go to Definition） | **转到 Java 定义**：引用 → 声明；无匹配时自动回退到转到引用 |
| `F12` | **转到 Java 定义**（`springNav.goToImplementation`），与 Ctrl/Cmd+点击一致，避免与原生 F12 语义产生歧义 |
| `Shift+F12` / 右键「查找所有引用」 | 原生 Go to References（已注册 `ReferenceProvider`）= **转到 Java 引用**，多结果以 peek 列表展示 |
| 右键菜单（Java / XML 文件） | 第一项「转到 Java 引用」，第二项「转到 Java 定义」 |

- 光标可放在**调用点**（如 `userService.selectUserList(user)`）或**声明**上，两个方向都可用。
- 右键菜单文案不含「向上/向下」字样。
- VS Code **不允许扩展绑定「鼠标点击 + 修饰键」组合**，`Cmd/Ctrl+Option+Click` 之类无法注册；能区分方向的鼠标手势只有内置 `Ctrl/Cmd+Click`。

### 自定义快捷键（可选）

插件默认只占用 `F12` 一个键盘键，不额外占用组合键。若需更多键位，可在 **Keyboard Shortcuts** 搜索命令或在 `keybindings.json` 自行绑定：

```json
[
  { "key": "ctrl+alt+j", "command": "springNav.goToImplementation", "when": "editorTextFocus" },
  { "key": "ctrl+alt+k", "command": "springNav.goToReference", "when": "editorTextFocus" }
]
```

### 实时接口路径搜索

命令面板执行 `SpringNav: 按接口路径搜索` / `SpringNav: Search by URL path`，打开实时列表：打开时为空，输入即过滤（无需回车），清空恢复为空；每行显示完整路径、文件名、HTTP 方法、模块名；支持多关键词（全部命中优先，否则任一命中）；回车打开。

### 索引与多项目

- 状态栏 `SpringNav: N 文件`，悬停看项目/模块/方法/边数量；**点击状态栏**或执行 `SpringNav: 重新加载索引` 全量重建。
- **单项目重建**：资源管理器右键项目/模块目录 → `SpringNav: 重建此项目索引`，其它项目索引不变。
- **多根工作区 / 多模块项目**：全部索引，同一项目或嵌套根只扫一次；模块名显示为模块相对路径（如 `ruoyi-admin`、`proj-a/mod-web`）。
- **保存文件**：只重解析被保存的 java/xml 文件，内存重建索引，不重新扫盘。
- **外部修改**（`git pull`、其它编辑器改动）不会自动感知，需手动重建；插件监听的是工作区内的保存事件与配置文件变更。

---

## 配置

在工程根目录放 `.idea/spring-nav.toml`（或 `.idea/spring-nav.json`）。完整示例见 [`.idea/spring-nav.toml`](.idea/spring-nav.toml)，JSON schema 见 [`schemas/spring-nav.schema.json`](schemas/spring-nav.schema.json)。

```toml
# 优先级：aliases > type-roles > 默认命名约定

[aliases]
# 字段名 / 类型名 -> 目标类型（简单名或全限定名），优先级最高
# "userService" = "ISysUserService"
# "ISysUserService" = "SysUserServiceImpl"   # 多实现时收敛到指定实现

[type-roles]
# 类名（简单名或全限定名）-> 角色
# 角色: controller | service | service_impl | mapper | entity | other
# "SysConfigController" = "controller"
# "com.ruoyi.system.service.ISysUserService" = "service"

[impl-patterns]
# 接口角色 -> 实现类文件名 glob
service = ["*ServiceImpl", "*Impl"]
mapper = ["*Mapper", "*Dao", "*Repository"]
```

- **`aliases`**：显式别名，用于字段类型无法从声明推断或把接口钉到特定实现。
- **`type-roles`**：强制指定角色。
- **`impl-patterns`**：找不到 `implements` 关系时按通配符匹配实现类；`*` / `?` 可用。

配置变更被 `FileSystemWatcher` 捕获，触发内存局部重建（复用已解析模型，不重新扫盘）。

### VS Code 设置

| 设置 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `springNav.defaultDirection` | `down` \| `up` \| `both` | `down` | `Ctrl/Cmd+点击`（Go to Definition 入口）采用的跳转方向；`down` = 转到定义，`up` = 转到引用 |
| `springNav.fallbackOnEmpty` | boolean | `true` | 指定方向无匹配时是否自动回退到相反方向一级 |

---

## 构建与多平台分发

### 通用前置

- Node ≥ 18、Rust stable（`rustup`）
- **Linux**：gcc / make
- **macOS**：Xcode Command Line Tools（`xcode-select --install`）
- **Windows**：Visual Studio Build Tools（MSVC，含 ARM64 交叉工具）+ Windows SDK

### 本机构建（当前平台）

```bash
npm install            # 仅开发依赖：typescript / @types/vscode / @types/node
npm run build:native   # cargo build --release -> native/spring-nav.<platform>.node
npm run compile        # tsc -> out/extension.js
npm run build          # 以上两步
npm run package        # 编译 + 构建 + 打包 VSIX
```

`package-lock.json` 锁定 3 个开发依赖的版本，CI 的 `package` job 使用 `npm ci`（必须有 lock 文件）；它随 `package.json` 一起提交，不要忽略或删除。

`scripts/build-native.sh` 把 `target/release/libspring_nav.{so,dylib,dll}` 复制并改名为 `native/spring-nav.<platform>-<arch>[-<libc>].node`（Windows 为 `...-msvc.node`）。`native/index.js` 依据 `process.platform` / `process.arch` / libc 在运行时**只 dlopen 一个** `.node`；其余架构文件仅为磁盘存在，不影响内存与运行效率。

大体积产物可用 `CARGO_TARGET_DIR` 重定向：

```bash
CARGO_TARGET_DIR=/tmp/springnav-target npm run build:native
```

### 单机双架构（`SN_TARGET`）

同一操作系统内可用 `SN_TARGET=<rust triple>` 交叉出另一架构，产物按三元组自动命名：

```bash
SN_TARGET=x86_64-apple-darwin      bash scripts/build-native.sh   # macOS 上出 darwin-x64
SN_TARGET=aarch64-pc-windows-msvc  bash scripts/build-native.sh   # Windows x64 上出 win32-arm64
SN_TARGET=aarch64-unknown-linux-gnu bash scripts/build-native.sh  # Linux x64 上出 linux-arm64（需 gcc-aarch64-linux-gnu）
```

**跨操作系统仍需对应 runner**：macOS 需要 Apple SDK、Windows 需要 MSVC，不能在一台机器上产出另一操作系统的 `.node`。

### CI / CD 全自动发布（推荐）

[`.github/workflows/build.yml`](.github/workflows/build.yml) 定义四个 job：

| job | 内容 |
| --- | --- |
| `test` | clippy `-D warnings` + Rust 单测 + `npm run build:native` + smoke + 克隆 RuoYi-Vue 跑 harness |
| `build` | 3 类 runner、4 个矩阵项产出 **6 个架构** `.node`（见下），各自上传 artifact |
| `package` | 汇集全部 `.node`，`npm ci` + `compile` + `vsce package` 打出**单个全平台通用 VSIX** |
| `release` | `tag v*` 时自动创建 GitHub Release 并上传 VSIX（`permissions: contents: write`） |

矩阵（同 runner 内 host 构建 + `SN_TARGET` 交叉）：

| runner | 产出 |
| --- | --- |
| `ubuntu-latest` | `linux-x64-gnu`（host）、`linux-arm64-gnu`（交叉，装 `gcc-aarch64-linux-gnu`） |
| `macos-latest`（arm64） | `darwin-arm64`（host）、`darwin-x64`（`SN_TARGET=x86_64-apple-darwin` 交叉） |
| `windows-latest`（x64） | `win32-x64-msvc`（host）、`win32-arm64-msvc`（`SN_TARGET=aarch64-pc-windows-msvc` 交叉） |

触发方式：

- push `v*` tag：全平台构建 + 打包 + **自动发 Release**
- PR（改动 `rust/` `scripts/` `test/` `package.json` `.github/workflows/`）：跑 `test` 门禁
- Actions 页面手动 `workflow_dispatch`

> 平台注意：**linux musl 变体不做**——Rust 的 `x86_64-unknown-linux-musl` 不支持编译 `cdylib`（musl libc 无动态加载），napi 原生模块无法用它构建；**macos-13 Intel runner 已退役**，x86_64 macOS 产物改由 arm64 runner 交叉编译。

### 全平台 VSIX

```
native/spring-nav.linux-x64-gnu.node
native/spring-nav.linux-arm64-gnu.node
native/spring-nav.darwin-x64.node
native/spring-nav.darwin-arm64.node
native/spring-nav.win32-x64-msvc.node
native/spring-nav.win32-arm64-msvc.node
```

```bash
npm run package     # 把上面所有 .node 一起打进 spring-nav-<version>.vsix
```

`package.json` 的 `napi.targets` 列出支持的目标三元组；`.vscodeignore` 不影响 `native/*.node` 与 `out/` 进包（已实测）。

### 分发

- 应用市场：`npx @vscode/vsce publish`（需 `publisher` + Marketplace PAT）。
- 离线：分发 `.vsix`，用户执行 `code --install-extension spring-nav-<version>.vsix`。
- 正式发布走 CI：版本号递增 → 提交推送 → 打 `v*` tag → 云端自动出包并发布 Release。
- macOS：若本地调试时 `.node` 被 Gatekeeper 隔离，执行 `xattr -d com.apple.quarantine <file>.node`；正常安装 VSIX 一般无需处理。云端产物未做 Apple 签名/公证。

---

## 测试与性能基线

### 测试套件

```bash
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings  # 0 警告
cargo test --manifest-path rust/Cargo.toml                                 # 17/17
node scripts/smoke.js                                                       # 73/73
node test/harness.js /path/to/RuoYi-Vue                                      # 28/28
npm run compile && npm run package                                           # TS 编译 + VSIX 打包
```

- `smoke.js` 自建 Controller/Service/Impl/Mapper/XML/Entity 齐全的临时工程，覆盖全链路跳转、Lombok、重载、继承、多根、增量、单项目重建、配置热加载、路径搜索；另含**独立上下文夹具**的 11 条专项断言：同名类不混入（常量/静态/方法引用/类型名点击）、祖父类方法跳转、就近覆盖优先、`@Autowired`/`@Qualifier` 跳转、跨文件边增量失效与恢复。
- `harness.js` 对 [RuoYi-Vue](https://github.com/yangzongzhuan/RuoYi-Vue) 真实项目验证定义/引用双向链、回退、实体字段 ↔ get/set、Lombok 合成访问器、实时路径命中元数据、配置与增量。
- 两者都直接加载 `.node`，不依赖 VS Code / GUI。
- 另有内存资源专项复测：100 次全量重建、5 轮压力（每轮 1000 次保存 + 10 万次查询）、中文多字节边界、`lastPanic` 检查。

### 基线数据（RuoYi，210 文件 / 190 Java / 166 XML 语句 / 1866 方法 / 4448 调用点 / 6420 边 / 7112 span）

两遍完整验收实测：

| 指标 | 第一遍 | 第二遍 | 判定 |
| --- | --- | --- | --- |
| 全量索引 | 78.92 ms | 82.25 ms | 无感（原基线 76-98 ms） |
| 单次查询 | 1.16 µs | 1.17 µs | 微秒级 |
| 保存 / 增量 | 4.198 ms | 3.857 ms | 毫秒级 |
| 索引后进程 RSS | 147.6 MB | 147.5 MB | 稳定 |
| 100 次全量重建后 RSS | 147.9 MB | 148.0 MB | +0.3~0.5 MB |
| 压力 5 轮 RSS 增量 | `12.2 → 0.1 → 0.1 → 0 → 0` MB | `11.9 → 0.5 → 0.1 → 0 → 0` MB | **收敛归零** |
| V8 `heapUsed`（每轮 GC 后） | 3.5 MB 恒定 | 3.5 MB 恒定 | 无 JS 对象堆积 |
| 索引计数器增长（100 次重建 + 5000 次保存 + 50 万次查询后） | **零** | **零** | 无泄漏 |
| `skipped` / `failed` / `xml_cache` | 0 / 0 / 0 | 0 / 0 / 0 | 无资源浪费 |

**内存口径说明**：RSS 是整个 Node 进程（V8 + libuv + Rust 索引 + glibc malloc arena），不是「索引占 147MB」；V8 堆仅 3.5MB，索引数据结构净大小约 30MB 量级。压力阶段一次性约 +12MB 是 `updateFile` 反复重建状态时 allocator arena 扩张、drop 后未立即归还 OS（Linux glibc 行为），随后 5 轮增量归零即证明有界、非泄漏。`call_cache(4448) == calls(4448)`、`edge_contrib(210) == files(210)` 说明无冗余缓存与孤立贡献。与 0.3.2 基线（约 156MB）相比无退化。

---

## 演示截图（RuoYi-Vue 实机，本地文件）

以下截图保存在本地 `screenshots/` 目录，**已加入 `.gitignore`，不提交到仓库**，也不会打进 VSIX。截图来自历史版本，菜单文案以当前版本为准。

| 文件 | 说明 |
| --- | --- |
| `screenshots/01-open.png` | 打开工程，光标在 Controller 的 `userService.selectUserList` 调用点 |
| `screenshots/02-ctrlclick-down.png` | Ctrl+点击：调用点 → Service 接口方法（转到定义） |
| `screenshots/03-down-service-to-impl.png` | 接口方法 → 实现类方法（转到定义） |
| `screenshots/04-down-impl-to-mapper.png` | 实现类 → Mapper 方法 |
| `screenshots/05-down-mapper-to-xml.png` | Mapper 方法 → XML 语句 |
| `screenshots/06-up-xml-to-mapper.png` | XML → Mapper（转到引用） |
| `screenshots/07-up-service-to-controller-peek.png` | 接口方法 → 控制器调用点（多结果 peek） |
| `screenshots/08-entity-getter-to-field.png` | `getXX` → 实体字段 |
| `screenshots/09-entity-field-to-accessors.png` | 字段 → get/set（peek 列表） |
| `screenshots/10-context-menu.png` | 右键菜单：`转到 Java 引用` / `转到 Java 定义` |
| `screenshots/11-context-menu-jump.png` | 右键「转到 Java 定义」实际跳转 |
| `screenshots/12-search-empty.png` ~ `14-search-slash-fuzzy.png` | 路径搜索：空列表 / 模糊检索 / 斜杠命中 |
| `screenshots/15-lombok-getter-to-field.png` | Lombok `@Data`：`getUserName()` → 实体字段 |
| `screenshots/16-status-bar.png` ~ `21-reindex-folder-done.png` | 状态栏、重建命令、多根、单项目重建提示 |

---

## 已知局限 / 明确不做

- **准确率优先于覆盖率**：同名类歧义时返回空（不跳），不做猜测式匹配。
- **JDK / 第三方库方法定义**：无源码、不索引外部依赖（RuoYi 实测约占调用点 16.9%，是覆盖率天花板的主因）。
- **动态代理 / AOP / 反射 / 运行时多态**：不识别 JDK Proxy、MyBatis 动态代理、`SpringUtils.getAopProxy(this)` 等间接调用。
- **完整类型推断不做**：链式调用（`lambdaUpdate().set(...)`）、泛型元素类型（`List<User>` 的 `get(0).getName()`）、表达式接收者（`getService().select()`）、重载按实参类型消歧——均因需要假设返回类型、有误跳风险，暂不实现。
- **多实现 / 多重载**：返回候选列表供选择，不做「唯一正确」推断。
- **Lambda 内部调用**、**非标准写法**：不保证准确；解析器面向常见 SpringBoot 写法。
- **注解跳转的静态局限**：`@Qualifier` 按精确简单名 / Spring 默认首字母大写 bean 名匹配，命名不一致时「点了没反应」；`@RequestMapping` 路径字符串点击（需类级+方法级拼接）暂未实现；`@Value("${...}")` → yml/properties 不做（会引入新文件类型与索引开销）。
- **Lombok**：支持类级/字段级 `@Data`、`@Getter`、`@Setter`、`@Value` 合成 getter/setter；`@Accessors(fluent = true)` 改名写法、生成的构造器 / `equals` / `toString` 不参与导航；已有同名显式方法时以显式方法为准；`static` 字段不生成访问器、`final` 字段不生成 setter。
- **鼠标组合手势**：VS Code 未开放「鼠标点击 + 修饰键」给扩展 API，`Cmd/Ctrl+Option+Click` 无法绑定。
- **外部修改不自动感知**：`git pull`、其它编辑器改动后需手动重建（`FileSystemWatcher` 只覆盖工作区保存事件与配置文件变更）。
- **与 Red Hat Java 共存**：两个 Provider 的结果可能合并展示，本插件不负责过滤其它 Provider 的结果。
- 不做语法高亮、补全、诊断、重构、格式化；**不替代 Red Hat Java**。
- **平台限制**：linux musl 不支持 `cdylib`；macos-13 Intel runner 已退役；云端产物未做 Apple 签名/公证。

---

## 目录结构

```
SpringNav/
├── package.json / package-lock.json / tsconfig.json / .vscodeignore / .gitignore
├── package.nls.json / package.nls.zh-cn.json   # 中英国际化文案
├── src/extension.ts            # TS 壳（零逻辑）、防御加载、Provider 注册
├── src/i18n.ts                 # 运行时提示文案
├── native/index.js             # 平台选择加载器
├── native/*.node               # 6 架构二进制（.gitignore 忽略，CI 产出）
├── scripts/build-native.sh     # 构建 .node，支持 SN_TARGET 跨架构
├── scripts/smoke.js            # 合成工程自检（73 项）
├── test/harness.js             # RuoYi 端到端断言（28 项）
├── rust/src/                   # napi-rs crate：scanner/parser/model/index/resolve/config/lib
├── .idea/spring-nav.toml       # 配置示例
├── schemas/spring-nav.schema.json
├── .github/workflows/build.yml # CI：test / build / package / release
└── screenshots/                # 本地演示截图（不入库、不打进 VSIX）
```

---

## 迭代指引

新增跳转场景或修 bug 时按以下流程，**任何一步都不能跳过**：

1. **先分析再动手**：评估复杂度、对性能/内存/资源占用的影响、精度风险与副作用，把方案摆出来确认口径后再实现。硬性约束：不引入 JVM / 语言服务 / AST / 外部源码下载；准确率优先（歧义返回空）。
2. **实现**：解析层扩展要沿用「字节等长掩码 + `col()` 边界兜底」；新边必须从 `compute_file_edges` 产出（走标准 k0/k1/k2），以便 `edge_contrib` 自动纳入增量失效；不要另建旁路缓存。
3. **全量回归**：clippy `-D warnings`、cargo test、`node scripts/smoke.js`、`node test/harness.js`、`npm run compile`、`npm run package`。新场景必须补对应断言（含中文/多字节边界、同名类歧义、跨文件边失效与恢复）。
4. **基线复测**：全量索引耗时、查询耗时、增量耗时、100 次重建收敛、压力轮 RSS 增量归零、索引计数器零增长，与本文档基线对比，确认无退化。
5. **提交推送**：`package.json` 版本号递增 → 提交推送 master → 打 `v<version>` tag → CI 自动出 6 架构产物、打全平台 VSIX 并发布 Release。
6. **同步文档**：功能清单、测试结果、性能基线、已知局限、版本号随改动一起更新到本 README。

关键改动位置速查：

| 需求 | 文件 |
| --- | --- |
| 新增语法捕获 / 类型表 | `rust/src/parser.rs` + `rust/src/model.rs` |
| 新增跳转边 / 解析精度 / 增量失效 | `rust/src/index.rs`（`compute_file_edges`、`resolve_type_files_ctx`、`apply_changes`） |
| 查询兜底 / 方向语义 | `rust/src/resolve.rs` |
| 命令、键位、右键菜单、Provider、文案 | `package.json`、`package.nls*.json`、`src/extension.ts`、`src/i18n.ts` |
| 构建产物命名 / 跨架构 | `scripts/build-native.sh`、`native/index.js` |
| CI 矩阵与发布 | `.github/workflows/build.yml` |

---

## License

MIT
