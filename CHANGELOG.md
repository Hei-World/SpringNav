# 更新日志

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循语义化版本。更早版本见 [Releases](https://github.com/Hei-World/SpringNav/releases)。

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
