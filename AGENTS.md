# SpringNav 项目上下文

本文件是维护者与后续 Agent 的详细上下文：功能清单、解析规则、测试基线、发布流程与项目进度。面向用户的简明说明见 [README.md](README.md)。

## 1. 项目定位与硬约束

- VS Code / Qoder 插件，只做 SpringBoot + MyBatis 工程的**跨文件跳转**：转到定义、转到引用、按接口路径搜索。
- 不做语法高亮、补全、诊断、重构、格式化；不替代 Red Hat Java。
- **硬约束（不可违背）**：
  1. 性能与内存优先：全量索引毫秒级、查询微秒级、保存增量毫秒级；
  2. 不引入 JVM、Java 语言服务（JDT LS / Red Hat Java）、tree-sitter、WASM、外部源码下载；
  3. 不构建完整 AST，不保存源码与语法树，只保存解析完成的边与紧凑元数据；
  4. 准确率优先于覆盖率：同名类/多实现/多重载有歧义时返回空，不猜测；
  5. 查询路径不读磁盘、不重新解析文件。
- 当前版本 `0.3.8`；MIT 协议；仓库公开但**关闭 Pull Request**，只接受 Issue / Discussions 反馈。

## 2. 当前仓库状态（2026-11）

- GitHub：`Hei-World/SpringNav`，public；Issues 与 Discussions 开启；Pull requests 关闭；Wiki / Projects 关闭。
- 分支：仅 `master`；规则集 `protect-master` 禁止删除、禁止强推；管理员 bypass。无 `develop`，无外部 PR。
- 依赖自动化：`dependabot.yml` 已删除，自动安全修复 PR 已关闭；Dependabot 漏洞告警仍保留。
- CI：`.github/workflows/build.yml`，tag `v*` 触发全流程并自动发布 Release；`workflow_dispatch` 全流程但不发 Release；PR 触发已移除。
- 文档分工：README 面向用户（简短）；本文件承载详细清单、基线与流程；`CONTRIBUTING.md` 反馈与开发约定；`CHANGELOG.md` 版本记录；`SECURITY.md` 漏洞报告。

## 3. 架构与数据流

```
VS Code (TypeScript 壳, 零逻辑) ──同步调用──> spring-nav.<platform>.node (Rust / napi-rs)
  activate / DefinitionProvider / ReferenceProvider / Command / Watchers
  仅：取当前行文本、传参、把 Loc 转成 vscode.Location

Rust:
  scanner.rs  枚举文件、相关性过滤、import 闭包二次扫描、>1MB 跳过
  parser.rs   逐行扫描 + 状态机 + 正则；字节等长掩码；col() 边界兜底
  config.rs   每项目 .idea/spring-nav.toml(.json)
  model.rs    紧凑元数据（MethodDecl / FieldDecl / CallSite / TypeRef / VarDecl / FieldRef / MethodRef / Qualifier）
  index.rs    HashMap 邻接表 + span 注册 + 预计算 down/up + 增量失效
  resolve.rs  纯内存查表、方向语义、行文本兜底、路径模糊搜索
  lib.rs      #[napi] 导出、AsyncTask、catch_unwind、lastPanic
```

### 3.1 边模型

| kind | 含义 | 用途 |
| --- | --- | --- |
| k0 | 无向边 | 双向可达、无需方向的关联 |
| k1 | 有向边（`down` + `up` 两个视图） | 定义 ↔ 引用；同一组边双向查询 |
| k2 | 别名边 | 变量 → 字段声明、注入字段 → 实现类、`@Qualifier` → 类 |

- 所有边由 `compute_file_edges` 产出并写入 `edge_contrib`（每文件贡献），增量失效据此定向重算；不要另建旁路缓存。
- `down`：引用 → 声明（调用 → 方法定义、接口方法 → 实现类、Mapper → XML）；`up`：声明 → 引用方（方法 → 调用点、字段 → get/set、XML → Mapper）。
- 方向缺失时默认回退相反方向一级（`springNav.fallbackOnEmpty`）。

### 3.2 索引与查询

- `buildIndex` 走 napi `AsyncTask`（libuv 线程池），不阻塞扩展激活；查询用 `RwLock` 读锁。
- 查询：`(uri,line)` → span → 命中符号 → 方向表 → 去重排序；纯内存。
- 列号：UTF-16 语义；`col()` 做 char boundary floor；字符串掩码按**字节等长**替换，中文不会导致偏移错位或 panic。
- 稳定性：`preflightNative` 用 `ELECTRON_RUN_AS_NODE` 子进程预加载 `.node`；所有 napi 入口 `catch_unwind`；`indexStats().lastPanic` 上报。

### 3.3 增量失效

- `edge_contrib`：文件产出的边；文件变脏或其边目标变脏 → 重算该文件。
- `loc_owner`：位置归属，用于反查贡献方。
- `call_cache`：调用点解析缓存（targets / from_lombok / field_based）。
- `impls_cache`、`xml_cache`：实现类与 XML 匹配缓存，变更时按需清理。
- 保存文件只重解析该 java/xml（250ms 防抖、批量合并），不扫盘。

## 4. 定义与引用语义

| 概念 | 含义 | 典型场景 |
| --- | --- | --- |
| 转到 Java 定义（down） | 引用 → 声明 | Controller 调用 → Service 接口方法 → 实现类 → Mapper → XML；getter → 字段；类型名/常量 → 声明 |
| 转到 Java 引用（up） | 声明 → 引用方 | 方法 → 所有调用点；字段 → get/set；XML → Mapper；接口方法 → 实现类 + 调用方 |

- 不存在声明 → 声明链：接口方法声明本身既是「定义」，也是「引用」查询的起点。
- 光标放在调用点或声明上都可用；多结果用 VS Code peek 列表。
- 入口：`Ctrl/Cmd+Click`、`F12`（绑定 `springNav.goToImplementation`）→ 定义；`Shift+F12`、右键「转到 Java 引用」→ 引用；右键第一项引用、第二项定义。
- VS Code 不开放「鼠标点击 + 修饰键」组合，`Ctrl/Cmd+Option+Click` 无法注册。

## 5. 功能清单

### 5.1 索引与工程

| 编号 | 功能 |
| --- | --- |
| F-01 | 检测 `pom.xml` / `build.gradle(.kts)` 后激活 |
| F-02 | 相关性过滤（Controller / Service / ServiceImpl / Mapper / Entity / `*Mapper.xml` 等）+ import 闭包二次扫描（工具类、常量类也会入索引） |
| F-03 | 异步惰性建索引，不阻塞激活 |
| F-04 | 真增量：只重解析变更文件，边与缓存定向失效 |
| F-05 | 状态栏 `SpringNav: N 文件`；悬停显示项目/模块/方法/边；点击全量重建 |
| F-06 | 多根工作区 / 多模块：每项目只扫一次，嵌套根去重；右键可单项目重建 |
| F-07 | 项目级配置生效、热加载、局部重建 |
| F-08 | 中英国际化（`package.nls*.json` + `src/i18n.ts`） |
| F-09 | 防御加载 + panic 全线封锁；原生模块损坏时给诊断而非拖死宿主 |
| F-10 | >1MB 文件跳过并计数（`skipped`） |

### 5.2 核心跳转

| 编号 | 功能 |
| --- | --- |
| J-01 | 方法调用 → 定义（Controller→Service、ServiceImpl→Mapper、同类/父类方法）；返回类型支持泛型、嵌套泛型、通配符、数组、限定名，含 `Map<String, Object>` 这类泛型内空格 |
| J-02 | Service 接口方法 ↔ 实现类方法（多实现给候选） |
| J-03 | Mapper 方法 ↔ XML 语句：精确 `(namespace, id)`，已移除模糊前缀匹配 |
| J-04 | 实体字段 ↔ get/set：手写、`getXxx` / `isXxx` / `setXxx`、Lombok 合成 |
| J-05 | 声明 → 所有引用方（`ReferenceProvider` / `Shift+F12`） |
| J-06 | 常量 / 静态变量 / 枚举引用 → 字段声明（`Class.CONST`、静态导入、裸 `ALL_CAPS`） |

### 5.3 调用点语义

| 编号 | 功能 |
| --- | --- |
| P1-1 | `this.xxx()` 与同类裸调用：沿继承链 BFS，就近覆盖优先 |
| P1-2 | 静态调用：大写接收者按 imports → 同包 → 唯一简单名 |
| P1-3 | 参数/局部变量接收者 → `@Data` 实体字段 |
| P1-4 | 点击接收者变量名 → 字段声明（k2） |
| P1-5 | 手写 get/set → 实体字段（任意角色） |
| P2-1 | `Class::method` → 方法定义或 Lombok 字段 |
| P2-2 | lambda 参数、for-each 变量类型解析 |
| P2-3 | 静态导入常量、枚举常量、裸常量 |
| P2-4 | 点击类型名 → 类声明（参数/局部/字段、`new X()`、`extends`/`implements`） |

### 5.4 精度与消歧

| 编号 | 功能 |
| --- | --- |
| A2 | `resolve_type_files_ctx`：imports → 同包 → 全项目唯一简单名；歧义返回空；覆盖调用点、常量、方法引用、静态调用、类型名、继承上溯、行文本兜底 |
| A3 | 多层继承：`find_methods` BFS（visited 环检测、深度上限 8），最近覆盖胜出 |
| A6a | `@Autowired`/`@Resource`/`@Inject` 接口字段 → 唯一实现类；`@Qualifier("bean")`/`@Resource(name="bean")` → 类声明（精确简单名或首字母大写 bean 名，要求唯一） |

### 5.5 路径、结果与配置

| 编号 | 功能 |
| --- | --- |
| S-01 | `springNav.searchByPath` 实时搜索：输入即过滤，显示完整 URL + 文件 + HTTP 方法 + 模块，多关键词 |
| S-02 | 类级 + 方法级 mapping 拼接完整 URL；支持相对路径（`"getInfo"`）、类路径尾斜杠归一化、方法无路径时取类路径、同一方法多 verb 分行；常量/表达式路径不猜测 |
| M-01 / M-02 | peek 多结果；去重 + 按路径/行号排序 |
| C-01 / C-02 | 命名约定；`.idea/spring-nav.toml`/`.json` 支持角色覆盖、实现类模式、显式别名 |
| C-03 / C-04 | 优先级：显式别名 > 角色覆盖 > 默认约定；配置热加载 |

## 6. 解析与精度细则

- 方法声明正则 `RE_METHOD` 的返回类型支持：限定名、嵌套泛型（`List<Map<String,Object>>`）、通配符（`Class<? extends Job>`）、数组、泛型方法前缀（`<T> T`、`<E extends Enum<E>> E`）。0.3.5 修复了泛型内空格导致整行匹配失败的问题。
- 参数类型、字段类型统一经 `strip_to_simple` 取简单名；`Map<String, Object>` 解析为 `Map`。
- XML 只做精确 `(namespace, id)`；不做前缀/相似匹配。
- 路径收集只取 mapping 注解的 `value` / `path` / 位置参数，忽略 `name` / `params` / `headers` / `consumes` / `produces`，避免 `application/json` 被当成 URL。
- 方法只有 mapping 注解、没有路径时，以类路径为 URL 生成条目；一个方法声明多个 verb 时每个 verb 一条；去重键为 `(url, loc, verb)`。
- 路径参数是常量/表达式（无字符串字面量）时标记 `path_unresolved`，跳过该类条目，不用类路径兜底。
- 注解参数按 `,` 切分时把 `{}` 也计入嵌套深度，`method = {GET, POST}` 不会被拆成两个参数。
- Lombok：类级/字段级 `@Data`、`@Getter`、`@Setter`、`@Value`；`static` 不生成访问器、`final` 不生成 setter；显式方法优先。
- `@Qualifier` 字符串 span 已注册，可直接点击；`@RequestMapping` 路径字符串点击未实现。
- `resolve_type_files_ctx` 是唯一类型解析入口；不要恢复 `resolve_class` 之类的宽松兜底。

## 7. 配置

优先级：**aliases > type-roles > impl-patterns > 默认命名约定**。

```toml
[aliases]
# "userService" = "ISysUserService"
# "ISysUserService" = "SysUserServiceImpl"

[type-roles]
# "SysConfigController" = "controller"
# 角色: controller | service | service_impl | mapper | entity | other

[impl-patterns]
service = ["*ServiceImpl", "*Impl"]
mapper = ["*Mapper", "*Dao", "*Repository"]
```

VS Code 设置：

| 设置 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `springNav.defaultDirection` | `down` / `up` / `both` | `down` | Ctrl/Cmd+点击采用的跳转方向 |
| `springNav.fallbackOnEmpty` | boolean | `true` | 指定方向无结果时回退相反方向一级 |

## 8. 测试体系

```bash
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings   # 0 警告
cargo test --manifest-path rust/Cargo.toml                                  # 19/19
node scripts/smoke.js                                                       # 94/94
node test/harness.js /path/to/RuoYi-Vue                                     # 32/32
npm run compile
```

- `scripts/smoke.js`：临时合成工程，覆盖全链路跳转、Lombok、重载、继承、多根、增量、单项目重建、配置热加载、路径搜索；独立上下文夹具 31 条：同名类不混入、祖父类方法、就近覆盖、`@Autowired`/`@Qualifier`、跨文件边失效恢复、泛型返回值（G-01～G-05）、空路径/相对路径/多 verb/尾斜杠/常量路径搜索（S-03～S-10）、双项目同名 FQCN/XML/bean 隔离（X-00～X-07）。
- `test/harness.js`：RuoYi-Vue 真实项目；定义/引用双向、回退、字段 ↔ accessor、Lombok、路径元数据、配置、增量；含 `Map<String,String>` / `List<Map<String,Object>>` 返回类型的真实回归（G-01～G-04）。
- 两者直接加载 `.node`，不依赖 VS Code。
- 内存专项：100 次重建、12 轮压力（每轮 1000 次保存 + 10 万次查询）、中文边界、`lastPanic`。

### 基线（RuoYi：210 文件 / 190 Java / 166 XML 语句 / 1885 方法 / 4448 调用点 / 6451 边 / 7131 span）

| 指标 | 0.3.4 | 0.3.5 | 判定 |
| --- | --- | --- | --- |
| 全量索引（10 次均值） | 88.63 ms | 82.97 ms | 持平/更快 |
| 真实调用点解析 | — | 1.19 µs | 微秒级 |
| 保存 / 增量 | 4.198 / 3.857 ms | 4.197 ms | 毫秒级 |
| 100 次重建后 RSS | 149.2 MB | 149.9 MB | +0.7 MB（新增 19 个方法声明），无累积 |
| 压力 RSS 平台 | +14.3 MB | +13.8 MB | 收敛不再增长 |
| V8 heapUsed | 3.5 MB | 3.8 MB | 恒定 |
| 计数器 | 稳定 | `methods=1885` / `calls=4448` / `edge_contrib=210` | 零增长 |
| skipped / failed | 0 / 0 | 0 / 0 | 无浪费 |

0.3.6/0.3.7（路径搜索补全）与 0.3.8（多项目隔离）A/B 复测：旧/新二进制交错各 20 次，全量索引 84.67 / 84.74 ms（持平）；RuoYi 路径条目 117 → 132、`path_entries` 120 → 147（新增均为真实无路径端点与相对路径）；50 次重建后 RSS 146.6 MB，5 轮压力增量 `11.0 → 11.7 → 11.9 → 11.9 → 11.9` MB 收敛，`heapUsed` 3.4 MB，计数器稳定。 0.3.8 的 RuoYi 单项目统计与 0.3.7 完全一致（132 路径 / 147 条目 / 6451 边）；索引 81.31 ms vs 82.99 ms；30 次重建 RSS 149.6 MB、heap 3.4 MB，8 轮压力稳定在 +14.5 MB。

**口径**：RSS 是整个 Node 进程（V8 + libuv + Rust 索引 + glibc arena），不是索引大小；V8 堆约 3.5MB，索引净大小约 30MB。压力阶段一次性约 +12MB 是 allocator arena 扩张，多轮后不再增长即证明有界、非泄漏。

## 9. 已知局限 / 明确不做

- JDK / 第三方库定义不索引（RuoYi 中约占调用点 16.9%，覆盖率天花板主因）。
- 动态代理 / AOP / 反射 / 运行时多态不识别。
- 链式调用、泛型元素类型、表达式接收者、重载按实参消歧不做。
- 多实现/多重载返回候选，不推断唯一答案。
- Lambda 内部调用与非标准写法不保证。
- `@Value("${...}")` → yml/properties 不做；`@RequestMapping` 路径字符串点击不做。
- Lombok `@Accessors(fluent = true)`、构造器、`equals`/`toString` 不参与导航。
- 鼠标点击 + 修饰键组合无法绑定。
- 外部修改（`git pull`、其它编辑器）需手动重建。
- 与 Red Hat Java 共存时结果可能合并展示。
- linux musl 不支持 `cdylib`；macOS 云端产物未签名/公证。

## 10. 构建、CI 与发布

```bash
npm ci
npm run build:native    # cargo build --release → native/spring-nav.<platform>.node
npm run compile         # tsc → out/
npm run package         # 全平台 .node 一起打 VSIX
```

- `scripts/build-native.sh`：host 构建取 `target/release`，`SN_TARGET=<triple>` 取 `target/<triple>/release`，按三元组命名。
- 可重定向产物：`CARGO_TARGET_DIR=/tmp/springnav-target npm run build:native`。
- CI 矩阵：`ubuntu-latest`（linux-x64 host + linux-arm64 交叉）、`macos-latest`（darwin-arm64 host + darwin-x64 交叉）、`windows-latest`（win32-x64 host + win32-arm64 交叉），共 6 个 `.node`，合并为单个 VSIX。
- 触发：tag `v*` 全流程 + Release；`workflow_dispatch` 全流程不出 Release；普通 push 与 PR 不触发。
- 发布步骤：改 `package.json` 与 `package-lock.json`（两处 `version`）→ 更新 README/CHANGELOG → 推送 → 打 tag → CI 发布。
- `package-lock.json` 必须入库（`npm ci`）；`.vscodeignore` 不影响 `native/*.node` 与 `out/`。

## 11. 目录与关键文件

```
rust/src/parser.rs   语法捕获、RE_METHOD、掩码、参数/字段/调用解析
rust/src/index.rs    compute_file_edges、resolve_type_files_ctx、find_methods/find_impls、增量
rust/src/resolve.rs  方向语义、span 查询、fallback、路径搜索
rust/src/model.rs    FileModel 与各类元数据
rust/src/scanner.rs  文件枚举、相关性、import 闭包
rust/src/config.rs   项目配置解析
rust/src/lib.rs      napi 导出、AsyncTask、panic 防线
src/extension.ts     Provider/命令/状态栏/监听
package.json         命令、键位、菜单、napi targets
scripts/smoke.js     合成断言（78）
test/harness.js      RuoYi 断言（32）
```

## 12. 迭代流程（不可跳步）

1. **分析**：复杂度、性能/内存影响、精度风险、副作用；确认口径。
2. **实现**：新边必须走 `compute_file_edges`（k0/k1/k2）以纳入增量；解析保持字节等长掩码 + `col()`。
3. **回归**：clippy、cargo test、smoke、harness、compile；新场景补断言（中文边界、同名歧义、边失效恢复）。
4. **基线**：索引/查询/增量耗时、100 次重建、压力 RSS、计数器。
5. **发布**：版本递增（含 lock 两处）→ 提交推送 → tag → CI 出包 + Release。
6. **文档**：README 保持简短；详细清单/基线/进度更新到本文件；版本变更写 CHANGELOG。

## 13. 项目进度

- 0.3.1：修复中文多字节掩码导致的列偏移/panic；panic 封锁与预加载。
- 0.3.2：`resolve_type_files_ctx` 解决同名类混入；XML 精确匹配。
- 0.3.3：GitHub Actions 全平台出包与自动 Release。
- 0.3.4：A2/A3/A6a（上下文解析、多层继承、注解跳转）；发布流程稳定。
- 0.3.5：修复泛型返回类型（带空格/嵌套）方法未入索引；新增单测与 smoke/harness 回归。
- 0.3.6：路径搜索补全——支持无方法路径（`@PostMapping` + 类路径）、相对路径、多 verb 分行、尾斜杠归一化；排除 produces/consumes 等非路径参数。
- 0.3.7：常量/表达式路径不再误落到类路径；注解数组参数按花括号整体切分。
- 0.3.8：多根工作区按项目隔离类型解析、实现类、XML、`@Qualifier` 与静态导入，修复跨项目同名文件误跳。
- 开源治理：仓库公开；关闭 PR，仅保留 Issue/Discussions；移除 Dependabot 自动 PR；master 仅禁删除/强推。

### 待办 / 可选方向

- 上架 VS Code Marketplace / Open VSX（需 publisher 与 PAT）。
- CodeQL 默认扫描（公开仓库可免费启用）。
- 视需要恢复 PR 流程时，同时恢复 `pull_request` 触发与必需检查。
- 若发布新版本，VSIX 内的 README 才会更新为当前文档。
