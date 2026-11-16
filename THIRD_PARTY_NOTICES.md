# 第三方 notices

SpringNav 以 MIT 许可证发布。发布产物中包含以下第三方组件的编译结果或运行时依赖，其许可证归原作者所有。

## 运行时 / 原生模块

| 组件 | 用途 | 许可证 |
| --- | --- | --- |
| [napi-rs](https://github.com/napi-rs/napi-rs)（`napi`、`napi-derive`、`napi-build`） | Node.js 原生模块绑定 | MIT |
| [regex](https://github.com/rust-lang/regex) | 解析器正则匹配 | MIT / Apache-2.0 |
| [toml](https://github.com/toml-rs/toml) | 读取 `.idea/spring-nav.toml` | MIT / Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 读取 `.idea/spring-nav.json` | MIT / Apache-2.0 |

## 开发依赖（不进入发布产物）

| 组件 | 用途 | 许可证 |
| --- | --- | --- |
| TypeScript | 编译 TS 壳 | Apache-2.0 |
| `@types/node`、`@types/vscode` | 类型定义 | MIT |
| VS Code Extension API | 宿主 API | MIT |

精确版本与完整依赖树见 `rust/Cargo.lock` 与 `package-lock.json`。

## 测试数据

`test/harness.js` 在本地或 CI 中按需克隆 [RuoYi-Vue](https://github.com/yangzongzhuan/RuoYi-Vue) 作为端到端测试工程。该项目仅用于测试，不随本仓库或发布产物分发，其许可证与版权归原作者所有。

## 商标

Spring、Spring Boot、Spring Cloud 是 VMware（Broadcom）的商标。SpringNav 是独立的第三方开源项目，与 VMware / Broadcom 无隶属或背书关系。Java 是 Oracle 的商标。VS Code 是 Microsoft 的商标。
