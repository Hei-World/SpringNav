# 更新日志

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循语义化版本。更早版本见 [Releases](https://github.com/Hei-World/SpringNav/releases)。

## [0.3.8] - 2026-11-16

### 修复

- **多项目跨跳**：多根工作区里两个项目存在同名类、相同 FQCN、相同 Mapper XML 命名空间/语句 id 或相同 bean 名时，跳转会落到另一个项目。现在所有类型解析、实现类查找、XML 匹配、`@Qualifier` 与静态导入均按引用文件所属项目过滤，跨项目同名符号不再互相干扰。Maven 多模块仍属于同一项目，模块之间跳转不受影响。
- `by_fqcn` 改为一对多（同一 FQCN 可存在于多个独立项目）；`impls_cache` / `xml_cache` 的 key 加入项目，避免缓存串项目。

### 测试

- Rust 单测 20/20；smoke 86 → **94/94**（新增 X-00～X-07：双项目同名 FQCN 接口、实现、引用、Mapper XML、bean 名、类型名，正反双向验证）。
- RuoYi 单项目 32/32，索引统计与 0.3.7 完全一致；索引 81.31 ms vs 82.99 ms（与 0.3.7 A/B 交错测量）。
- 30 次重建 RSS 149.6 MB、heap 3.4 MB；8 轮压力 RSS 稳定在 +14.5 MB，计数器零增长。

## [0.3.7] - 2026-11-16

### 修复

- 路径来自常量或表达式（`@GetMapping(ApiPaths.USER)`、`@GetMapping(value = PREFIX + "/x")`）时不再当成「无方法路径」落到类路径，避免生成错误的 `/chat` 条目；此类写法不生成路径条目。
- 注解参数按逗号切分时把 `{}` 计入嵌套深度，`method = {RequestMethod.GET, RequestMethod.POST}` 不再被拆成两个参数。

### 测试

- Rust 单测保持 20，新增常量路径断言；smoke 85 → 86（S-10）。
- RuoYi 数据与 0.3.6 一致：132 条路径 / 147 条目，无异常 URL。

## [0.3.6] - 2026-11-16

### 修复

- **无方法路径的接口搜不到**：类级 `@RequestMapping("/chat")` 配合裸 `@PostMapping` / `@PutMapping` / `@GetMapping` 时，现在会生成 `/chat` 条目，并按 `[GET]` / `[POST]` / `[PUT]` 分行。
- **相对路径被丢弃**：`@GetMapping("getInfo")` 这类不带前导 `/` 的写法现已正确拼接到类路径下。
- **多请求方式只留一个**：`@RequestMapping(method = {GET, POST})` 现在每个 verb 各一行，不再被去重合并。
- **非路径参数误判**：只收集 mapping 的 `value` / `path` / 位置参数，忽略 `name` / `params` / `headers` / `consumes` / `produces`，避免 `application/json` 变成 URL。
- 类路径尾斜杠统一归一化（`/chat/` → `/chat`）。

### 测试

- Rust 单测 19 → 20（映射参数解析）。
- smoke 78 → 85：新增 S-03～S-09（空路径、多 verb、相对路径、produces 噪声、尾斜杠、映射变更后的增量失效）。
- RuoYi：路径条目 117 → 132，`/getInfo`、`/getRouters` 恢复，`/` 只保留真实根映射。

## [0.3.5] - 2026-11-16

### 修复

- 方法声明的返回类型支持泛型内空格与嵌套泛型：`Map<String, Object>`、`List<Map<String, Object>>`、`Class<? extends Job>`、`<T> Map<String, T>`、`<E extends Enum<E>> E` 等此前整行匹配失败、方法未入索引，导致调用点与接口声明都跳不到实现。RuoYi 实测恢复 19 个真实方法声明，无新增误判。

### 测试

- 新增 2 条解析单测（6 种返回类型写法 + 局部变量反例）。
- smoke 新增 G-01～G-05，harness 新增 4 条 RuoYi 真实代码回归；总计 19 单测 / 78 smoke / 32 harness。

## [0.3.4] - 2026-11-14

### 新增

- **A2 上下文感知类型解析**：常量引用、静态调用、方法引用、类型名点击、继承上溯与行文本兜底统一走 `resolve_type_files_ctx`，同名类歧义时返回空。
- **A3 多层继承**：同类/父类方法调用沿继承链 BFS 上溯（环检测、深度上限 8），就近覆盖优先。
- **A6a 注解跳转**：`@Autowired` / `@Resource` / `@Inject` 接口字段 → 唯一实现类；`@Qualifier("bean")` / `@Resource(name="bean")` → 类声明。

### 变更

- 全平台发布改由 GitHub Actions 完成：tag `v*` 触发测试、6 架构 `\.node` 构建、单 VSIX 打包与 Release 发布。
- 移除 linux musl 变体（不支持 `cdylib`）与已退役的 `macos-13` runner，Intel macOS / Windows arm64 改为交叉编译。
- `package-lock.json` 纳入版本同步要求。

## [0.3.3] - 2026-11-13

### 新增

- GitHub Actions 全自动出包：`linux-x64/arm64`、`darwin-x64/arm64`、`win32-x64/arm64` 六个原生模块，合并为单个全平台 VSIX。
- Release 自动发布（`contents: write`）。

## 更早版本

0.3.2 及之前没有独立 tag（0.3.0～0.3.2 为持续迭代），主要修复包括中文多字节掩码导致的列号偏移与 `char boundary` panic、原生模块 panic 穿越 FFI 导致的扩展宿主重启、同名类混入、MyBatis XML 模糊前缀串扰等，详见提交历史。
