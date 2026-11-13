# SpringNav

面向 VS Code 的**轻量级 SpringBoot 代码导航插件**。它不运行 Java 语言服务、不做语法高亮/补全/诊断，只做一件事：建立跨文件跳转索引，让你在 SpringBoot / MyBatis 工程里流畅地「跳过去、跳回来」。

核心约束：**极致性能、最小内存占用**。TS 壳零逻辑，解析与匹配全部在 Rust 里完成，通过 napi-rs 编译为原生 `.node` 模块。

---

## 架构

```
VS Code (TypeScript 壳, 零逻辑)  ──同步调用──>  spring-nav.<platform>.node (Rust / napi-rs)
  activate / DefinitionProvider / Command / Watchers        ├─ scanner.rs   枚举文件
  仅: 取当前行文本、传参、把 Loc 转成 vscode.Location     ├─ parser.rs    逐行扫描 + 状态机 + 正则
                                                          ├─ config.rs    .idea/spring-nav.toml
                                                          ├─ model.rs    紧凑元数据结构体
                                                          ├─ index.rs    HashMap + 预计算邻接图
                                                          ├─ resolve.rs  查表 / 路径搜索
                                                          └─ lib.rs      #[napi] 导出
```

- **不启动 JDT LS、不依赖 Red Hat Java、不用 tree-sitter、不用 WASM**（P-01/P-04/P-06）。
- 解析器是 Rust 手写轻量解析器（逐行扫描 + 状态机 + 正则），**不构建完整 AST**。
- 索引**只存元数据**（类名、方法名、参数类型、文件 URI、行号、注解路径、字段类型），不存文件内容、不存 AST（P-02）。典型项目内存占用几十 MB 以内。
- 索引构建阶段把**所有跳转关系预计算成有向边（`down` / `up`）**，并登记光标 span。查询时只做「`(uri,line)` 找 span → 命中符号 → 查方向表 → 去重排序」，**纯内存查表，无磁盘 I/O、不重新解析文件**，Rust 侧微秒级（P-03）。
- **方向语义**：`down` = 控制器 → 接口方法 → 实现类 → Mapper → XML，以及 `getXX/setXX` → 实体字段；`up` = 逐级反向。多结果就是邻接表里的多个位置，TS 直接映射成 `Location[]`，VS Code 以 peek 列表展示（M-01）。
- `buildIndex` 用 napi `AsyncTask` 在 libuv 线程池异步构建，**不阻塞扩展激活**；查询走 `RwLock` 读锁。

---

## 支持的功能

| 编号 | 功能 |
| --- | --- |
| F-01 | Java 项目检测：工作区存在 `pom.xml` / `build.gradle` / `build.gradle.kts` 才激活 |
| F-02 | 惰性异步索引构建，只解析 Controller / Service / ServiceImpl / Mapper / Entity / `*Mapper.xml` 相关文件 |
| F-03 | 增量更新：监听保存事件，仅重新解析被保存的文件并在内存重算索引 |
| J-01 | Controller 方法 ↔ Service 接口方法 |
| J-02 | Service 接口方法 ↔ ServiceImpl 实现方法（多实现返回列表） |
| J-03 | ServiceImpl 方法 ↔ Mapper 接口方法 |
| J-04 | Mapper 接口方法 ↔ XML 语句（namespace + id 匹配，允许近似结果） |
| J-05 | 上述链路反向逐级回溯：XML → Mapper → ServiceImpl → Service 接口 → Controller |
| J-06 | 实体类字段 ↔ get/set 方法双向跳转 |
| J-07 | `this::method` 方法引用跳转（单重载精准，多重载返回列表） |
| J-08 | 重载处理：索引键含方法名 + 参数类型；命中多个返回列表 |
| J-09 | 继承链查找：当前类找不到时沿 `extends`/`implements` 上溯 |
| L-01 | Lombok：`@Data`/`@Getter`/`@Setter`/`@Value`（类级或字段级）合成 getter/setter，`getXxx()`/`isXxx()`/`setXxx()` → 实体字段 |
| S-01 | `springNav.searchByPath`：实时搜索列表，输入即过滤；显示 `路径 + (文件名) + [HTTP方法] + 模块` |
| S-02 | 路径索引：拼接类级 + 方法级 `@RequestMapping`/`@GetMapping` 等注解为完整 URL |
| M-01 | 多结果返回 `Location[]`，VS Code peek 列表选择 |
| M-02 | 结果去重 + 按文件路径与行号排序 |
| C-01 | 内置命名约定：`*Controller`、`*Service`/`*ServiceImpl`、`*Mapper`/`*Dao`、`*Repository` |
| C-02 | 读取 `.idea/spring-nav.toml`（或 `.json`）：类型角色覆盖 / 实现类命名模式 / 显式别名 |
| C-03 | 规则优先级：**显式别名 > 角色覆盖 > 默认命名约定** |
| C-04 | 配置文件变更时热加载并触发索引局部重建 |

### 用法

打开一个 SpringBoot 工程（含 `pom.xml` 等），插件自动激活并异步建索引。输出通道 `SpringNav` 会打印索引统计。

#### 方向跳转（向下 / 向上）

**向下**：控制器 → 接口方法 → 实现类 → Mapper → XML；`getXX/setXX` → 实体字段。
**向上**：上述链路逐级反向（XML → Mapper → 实现类 → 接口方法 → 控制器）。

| 入口 | 行为 |
| --- | --- |
| `Ctrl` / `Cmd` + 鼠标左键点击（内置 Go to Definition） | **向下**；该方向无匹配时自动**回退向上**一级（可配置） |
| `F12` | **向上**（覆盖了 VS Code 内置 F12 的 Go to Definition，仅 java/xml 生效） |
| 右键菜单 **转到Java实现** | 向下，直接跳转（多结果弹 peek 列表） |
| 右键菜单 **转到Java引用** | 向上，直接跳转（多结果弹 peek 列表） |

> 默认不额外注册其它键盘快捷键，避免与系统/其它插件冲突。如果确实需要，可自行在 `keybindings.json` 给 `springNav.goToImplementation` / `springNav.goToReference` 绑定任意键位。

- 光标可放在**调用点**（如 `userService.selectUserList(user)`）或**方法声明**上；两个方向都可用。
- 某个方向没有匹配时：默认回退相反方向一级；两方向都没有，VS Code 显示标准 `No definition found`（右键菜单/快捷键则提示「未找到匹配的…」）。
- 右键菜单只在 Java / XML 文件中显示，菜单文案不含「向上/向下」字样。

#### 自定义快捷键（可选）

插件默认只注册 `Ctrl/Cmd+点击`（向下）和 `F12`（向上）两个入口，不额外占用键盘组合，避免与系统或其它插件冲突。

如果你想要更多键位，`转到Java实现` / `转到Java引用` 是普通命令，可在 VS Code 的 **Keyboard Shortcuts**（`Ctrl+K Ctrl+S` / `Cmd+K Cmd+S`）里搜索并绑定，或在 `keybindings.json` 中自行添加，例如：

```json
[
  { "key": "ctrl+alt+j", "command": "springNav.goToImplementation", "when": "editorTextFocus" },
  { "key": "ctrl+alt+k", "command": "springNav.goToReference", "when": "editorTextFocus" }
]
```

> 说明：VS Code **不允许扩展绑定「鼠标点击 + 修饰键」组合**（`keybindings.json` 的 `key` 只接受键盘键）。所以 `Cmd+Option+Click` 这类手势无法注册；能区分方向的鼠标手势只有内置的 `Ctrl/Cmd+Click`（已用作「向下」），「向上」用 `F12` 或上述可配置快捷键。

#### 实时接口路径搜索

命令面板执行 `SpringNav: Search by URL path`（或绑定快捷键），打开**实时搜索列表**：

- **打开时列表为空，输入内容后才开始检索**（输入即过滤，无需回车）；清空输入则列表恢复为空。
- 每行显示**接口完整路径（控制器类级前缀 + 方法路径，如 `/sys` + `/list` = `/sys/list`）**，后接 `((文件名) [HTTP方法])` 与模块名，例如 `/system/user/list  ((SysUserController.java) [GET])  ruoyi-admin`（不再单列 `/list` 这种片段）。
- 支持多关键词（空格分隔，全部命中优先，若无结果则任一命中），如 `user list`。
- 回车打开选中项；不显示文件的绝对路径。

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

三段含义：

- **`aliases`**：显式别名。左边是字段名或类型名，右边是目标类型。用于字段类型无法从声明推断，或把接口钉到某个特定实现。优先级最高。
- **`type-roles`**：角色覆盖。把某个类强制指定为 `controller` / `service` / `service_impl` / `mapper` / `entity` / `other`。
- **`impl-patterns`**：实现类命名模式，`*` / `?` 通配符。找不到 `implements` 关系时按它匹配实现类。

配置变更会被 `FileSystemWatcher` 捕获，触发**内存局部重建**（复用已解析的模型，不重新扫盘）。

### VS Code 设置

| 设置 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `springNav.defaultDirection` | `down` \| `up` \| `both` | `down` | `Ctrl/Cmd+点击`（Go to Definition）的默认方向 |
| `springNav.fallbackOnEmpty` | boolean | `true` | 指定方向无匹配时是否回退相反方向一级 |

---

## 构建与多平台分发

### 通用前置

- Node ≥ 18、Rust stable（`rustup`）
- 平台额外依赖：
  - **Linux**：gcc / make；构建 musl 目标需 `musl-tools`
  - **macOS**：Xcode Command Line Tools（`xcode-select --install`）
  - **Windows**：Visual Studio Build Tools（MSVC）+ Windows SDK

### 本机构建（当前平台）

```bash
npm install            # 仅开发依赖：typescript / @types/vscode / @types/node
npm run build:native   # cargo build --release -> native/spring-nav.<platform>.node
npm run compile        # tsc -> out/extension.js
npm run build          # 以上两步
npm run package        # 编译 + 构建 + 打包 VSIX
```

`scripts/build-native.sh` 会把 `target/release/libspring_nav.{so,dylib,dll}` 复制并改名为
`native/spring-nav.<platform>-<arch>[-<libc>].node`（Windows 为 `...-msvc.node`）。
`native/index.js` 依据 `process.platform` / `process.arch` / libc(gnu/musl) 选择对应 `.node`。
大体积产物可用 `CARGO_TARGET_DIR` 重定向：

```bash
CARGO_TARGET_DIR=/tmp/springnav-target npm run build:native
```

### macOS 打包

```bash
xcode-select --install                    # 首次需要
rustup target add aarch64-apple-darwin    # Apple Silicon
rustup target add x86_64-apple-darwin     # Intel

npm install
npm run build:native                      # 产出 native/spring-nav.darwin-arm64.node（或 darwin-x64）
npm run package                           # 生成 spring-nav-0.1.0.vsix
```

同时支持 Apple Silicon + Intel（一个 VSIX 内含两个 `.node`）：

```bash
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
cp rust/target/aarch64-apple-darwin/release/libspring_nav.dylib native/spring-nav.darwin-arm64.node
cp rust/target/x86_64-apple-darwin/release/libspring_nav.dylib  native/spring-nav.darwin-x64.node
npm run package
```

> 也可以用 `lipo -create` 合成 universal `.node`；更简单的做法是各放一个，加载器按 `process.arch` 选择。

### Windows 打包

```powershell
rustup target add x86_64-pc-windows-msvc
npm install
npm run build:native      # native/spring-nav.win32-x64-msvc.node（建议在 Git Bash 下执行脚本）
npm run package
```

需要安装 VS Build Tools（C++ 工作负载）。ARM64 用 `aarch64-pc-windows-msvc`。

### Linux 打包

```bash
npm install
npm run build:native      # native/spring-nav.linux-x64-gnu.node
npm run package
```

musl（Alpine）：`rustup target add x86_64-unknown-linux-musl`，安装 `musl-tools`，产物为 `spring-nav.linux-x64-musl.node`。

### 交叉编译说明

- **不能在一个系统上直接产出另一个系统的 `.node`**：macOS 需要 Apple SDK、Windows 需要 MSVC，通常必须在本机构建（或用对应 runner 的 CI）。
- 推荐用 [`.github/workflows/build.yml`](.github/workflows/build.yml) 的矩阵（ubuntu / macos / windows）产出各平台 `.node`，下载 artifact 后统一放进 `native/`。

### 全平台 VSIX

一个 VSIX 可以同时包含多个平台的 `.node`，运行时由 `native/index.js` 自动挑选：

```
native/spring-nav.linux-x64-gnu.node
native/spring-nav.linux-arm64-gnu.node
native/spring-nav.linux-x64-musl.node
native/spring-nav.darwin-x64.node
native/spring-nav.darwin-arm64.node
native/spring-nav.win32-x64-msvc.node
native/spring-nav.win32-arm64-msvc.node
```

```bash
npm run package     # 把上面所有 .node 一起打进 spring-nav-<version>.vsix
```

### 分发

- 应用市场：`npx @vscode/vsce publish`（需 `publisher` + Marketplace PAT）。
- 离线：分发 `.vsix`，用户执行 `code --install-extension spring-nav-0.1.0.vsix`。

> macOS 小提示：若本地调试时 `.node` 被 Gatekeeper 隔离，可执行 `xattr -d com.apple.quarantine <file>.node`；通过 VS Code 正常安装 VSIX 一般无需处理。

`package.json` 的 `napi.targets` 列出了支持的目标三元组。

---

## 测试

```bash
node scripts/smoke.js                       # 合成最小 SpringBoot 工程，32 项断言（含 Lombok）
node test/harness.js /path/to/RuoYi-Vue     # 真实开源项目端到端断言（26 项）
```

`smoke.js` 自建一个 Controller/Service/Impl/Mapper/XML/Entity 齐全的临时工程；`harness.js` 对 [RuoYi-Vue](https://github.com/yangzongzhuan/RuoYi-Vue) 验证
Controller → Service → Impl → Mapper → XML 双向链、**向下/向上与回退**、实体字段 ↔ get/set、**Lombok 合成访问器**、**实时路径命中的文件名/HTTP 方法/模块名**、重载/继承、配置热加载与增量更新。两者都直接加载 `.node`，不依赖 VS Code。

---

## 演示截图（RuoYi-Vue 实机，本地文件）

以下截图保存在本地 `screenshots/` 目录，**已加入 `.gitignore`，不提交到仓库**（避免二进制文件入库），也不会打进 VSIX。需要查看时直接在项目根目录打开该文件夹。

| 文件 | 说明 |
| --- | --- |
| `screenshots/01-open.png` | 打开工程，光标在 Controller 的 `userService.selectUserList` 调用点 |
| `screenshots/02-ctrlclick-down.png` | `Ctrl+点击` 控制器 → Service 接口方法（向下） |
| `screenshots/03-down-service-to-impl.png` | 接口 → 实现类（向下） |
| `screenshots/04-down-impl-to-mapper.png` | 实现类 → Mapper（向下） |
| `screenshots/05-down-mapper-to-xml.png` | Mapper → XML 语句（向下） |
| `screenshots/06-up-xml-to-mapper.png` | `F12` XML → Mapper（向上） |
| `screenshots/07-up-service-to-controller-peek.png` | 接口 → 控制器（向上，多结果 Peek 列表） |
| `screenshots/08-entity-getter-to-field.png` | `getXX` → 实体字段（向下，直接跳转） |
| `screenshots/09-entity-field-to-accessors.png` | 字段 → get/set（向上，Peek 列表） |
| `screenshots/15-lombok-getter-to-field.png` | Lombok `@Data`：`getUserName()` → 实体字段（向下） |
| `screenshots/10-context-menu.png` | 右键菜单：`转到Java实现` / `转到Java引用` |
| `screenshots/11-context-menu-jump.png` | 右键「转到Java实现」实际跳转到字段 |
| `screenshots/12-search-empty.png` | 路径搜索打开时列表为空 |
| `screenshots/13-search-fuzzy.png` | 输入 `user list` 模糊检索，显示完整路径 |
| `screenshots/14-search-slash-fuzzy.png` | 输入 `/sys/user/list` 模糊命中 `/system/user/list` |

---

## 已知局限

- **多实现 / 多重载**：返回列表供选择（peek），不做「唯一正确」推断。
- **动态代理**：不识别 JDK Proxy、MyBatis 动态代理实现、`SpringUtils.getAopProxy(this)` 之类的间接调用；不做完整 Java 语义分析（类型推断、泛型解析、动态代理识别）。
- **Lambda 内部调用**：不保证跳转准确性（明确不做的事 2.7）。
- **与 Red Hat Java 共存**：两个 Provider 的结果可能合并展示，本插件不负责过滤其它 Provider 的结果。
- **非标准写法**：不保证准确；解析器面向常见 SpringBoot 写法，不处理所有边界语法。
- 不做语法高亮、代码补全、错误诊断、重构、格式化；**不替代 Red Hat Java**。
- `this::method` 仅在单重载时精准。
- **Lombok**：支持类级/字段级 `@Data`、`@Getter`、`@Setter`、`@Value` 的 getter/setter 合成，`entity.getXxx()` / `isXxx()` / `setXxx()` 会跳到实体字段。限制：`@Accessors(fluent = true)` 这类改名的写法、以及生成的构造器 / `equals` / `toString` 不参与导航；类里已有同名显式方法时以显式方法为准；`static` 字段不生成访问器，`final` 字段不生成 setter。
- **鼠标组合手势**：VS Code 未把「鼠标点击 + 修饰键」开放给扩展 API，`Cmd/Ctrl+Option+Click` 之类无法绑定；只有内置 `Ctrl/Cmd+Click` 可用（已用作向下），向上请用 `F12` 或自定义快捷键。

---

## 目录结构

```
SpringNav/
├── package.json / tsconfig.json / .vscodeignore / .gitignore
├── src/extension.ts            # TS 壳（零逻辑）
├── native/index.js             # 平台选择加载器 + *.node
├── scripts/build-native.sh     # 构建 .node
├── scripts/smoke.js            # 合成工程自检
├── test/harness.js             # RuoYi 端到端断言
├── rust/                       # napi-rs crate（解析器/索引/查询）
├── screenshots/                # 本地演示截图（.gitignore 忽略，不入库、不打进 VSIX）
├── .idea/spring-nav.toml       # 配置示例
├── schemas/spring-nav.schema.json
└── .github/workflows/build.yml # 多平台矩阵
```

## License

MIT
