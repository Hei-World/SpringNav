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
- 索引构建阶段把**所有跳转关系预计算成无向边**，并登记光标 span。查询时只做「`(uri,line)` 找 span → 命中符号 → 查邻接表 → 去重排序」，**纯内存查表，无磁盘 I/O、不重新解析文件**，Rust 侧微秒级（P-03）。
- 双向跳转靠「边两端都登记」天然实现；多结果就是邻接表里的多个位置，TS 直接映射成 `Location[]`，VS Code 以 peek 列表展示（M-01）。
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
| S-01 | `springNav.searchByPath` 命令：输入 `/v1/getInfo` 形式路径定位 Controller 方法 |
| S-02 | 路径索引：拼接类级 + 方法级 `@RequestMapping`/`@GetMapping` 等注解为完整 URL |
| M-01 | 多结果返回 `Location[]`，VS Code peek 列表选择 |
| M-02 | 结果去重 + 按文件路径与行号排序 |
| C-01 | 内置命名约定：`*Controller`、`*Service`/`*ServiceImpl`、`*Mapper`/`*Dao`、`*Repository` |
| C-02 | 读取 `.idea/spring-nav.toml`（或 `.json`）：类型角色覆盖 / 实现类命名模式 / 显式别名 |
| C-03 | 规则优先级：**显式别名 > 角色覆盖 > 默认命名约定** |
| C-04 | 配置文件变更时热加载并触发索引局部重建 |

### 用法

- 打开一个 SpringBoot 工程（含 `pom.xml` 等），插件自动激活并异步建索引。
- 把光标放在调用点或声明上，按 `F12` / `Ctrl+Click` 跳到另一端；命中多个时弹出 peek 列表。
- 命令面板执行 `SpringNav: Search by URL path`，输入 `/system/user/list` 定位接口方法。
- 输出通道 `SpringNav` 会打印索引统计（文件数、方法数、边数等）。

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

---

## 构建与多平台分发

### 本机构建（Linux x64 gnu）

前置：Node ≥ 18、Rust（stable）、gcc/make。

```bash
npm install           # 仅开发依赖：typescript / @types/vscode / @types/node
npm run build:native  # cargo build --release，产出 native/spring-nav.<platform>.node
npm run compile       # tsc -> out/extension.js
npm run build         # 两者都做
```

`scripts/build-native.sh` 会把 `target/release/libspring_nav.{so,dylib,dll}` 复制并改名为
`native/spring-nav.<platform>-<arch>-<libc>.node`（Windows 为 `...-msvc.node`）。
`native/index.js` 依据 `process.platform` / `process.arch` / libc(gnu/musl) 选择对应 `.node`。
大体积产物可用 `CARGO_TARGET_DIR` 重定向，例如：

```bash
CARGO_TARGET_DIR=/tmp/springnav-target npm run build:native
```

### 打包 VSIX

```bash
npm run package       # 编译 + 构建 + npx @vscode/vsce package --no-dependencies
```

注意：`.node` 是平台相关的二进制，需把目标平台的 `.node` 放进 `native/` 后再打包；`.vscodeignore` 已排除 `rust/`、`src/`、`test/` 等源码目录。

### 多平台 `.node`

本机只能产出当前平台的 `.node`。Windows / macOS / Linux 各架构由
[`.github/workflows/build.yml`](.github/workflows/build.yml) 的矩阵编译并上传 artifact：

| 平台 | 产物 |
| --- | --- |
| Linux x64 / arm64 gnu、x64 musl | `spring-nav.linux-x64-gnu.node` 等 |
| macOS x64 / arm64 | `spring-nav.darwin-x64.node` / `darwin-arm64.node` |
| Windows x64 / arm64 MSVC | `spring-nav.win32-x64-msvc.node` 等 |

分发做法：CI 产出各平台 `.node` → 放入 `native/` → 打包 VSIX（VSIX 可同时包含多个平台的 `.node`，加载器自动挑选），或按平台分别发布。`package.json` 的 `napi.targets` 列出了支持的目标三元组。

---

## 测试

```bash
node scripts/smoke.js                       # 合成最小 SpringBoot 工程，15 项断言
node test/harness.js /path/to/RuoYi-Vue     # 真实开源项目端到端跳转断言
```

`smoke.js` 自建一个 Controller/Service/Impl/Mapper/XML/Entity 齐全的临时工程；`harness.js` 对 [RuoYi-Vue](https://github.com/yangzongzhuan/RuoYi-Vue) 验证
Controller → Service → Impl → Mapper → XML 双向链、实体字段 ↔ get/set、路径搜索、重载/继承、配置热加载与增量更新。两者都直接加载 `.node`，不依赖 VS Code。

---

## 已知局限

- **多实现 / 多重载**：返回列表供选择（peek），不做「唯一正确」推断。
- **动态代理**：不识别 JDK Proxy、MyBatis 动态代理实现、`SpringUtils.getAopProxy(this)` 之类的间接调用；不做完整 Java 语义分析（类型推断、泛型解析、动态代理识别）。
- **Lambda 内部调用**：不保证跳转准确性（明确不做的事 2.7）。
- **与 Red Hat Java 共存**：两个 Provider 的结果可能合并展示，本插件不负责过滤其它 Provider 的结果。
- **非标准写法**：不保证准确；解析器面向常见 SpringBoot 写法，不处理所有边界语法。
- 不做语法高亮、代码补全、错误诊断、重构、格式化；**不替代 Red Hat Java**。
- `this::method` 仅在单重载时精准；Lombok `@Data` 等生成的方法源码里不存在，不会产生跳转目标。

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
├── .idea/spring-nav.toml       # 配置示例
├── schemas/spring-nav.schema.json
└── .github/workflows/build.yml # 多平台矩阵
```

## License

MIT
