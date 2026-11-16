## 改动说明

<!-- 这个 PR 解决什么问题？关联 Issue 请写 Fixes #123 -->

## 改动类型

- [ ] 修复跳转错误 / 返回空
- [ ] 新增跳转场景
- [ ] 解析精度 / 消歧改进
- [ ] 性能 / 内存优化
- [ ] 文档 / 测试 / 构建

## 影响评估（必须填写）

- 是否新增索引数据或边：<!-- 是/否；说明结构变化 -->
- 对全量索引耗时的影响：<!-- 本地实测，与 README 基线对比 -->
- 对内存/资源占用的影响：<!-- 100 次重建后 RSS、索引计数器 -->
- 是否有误跳风险：<!-- 歧义场景如何处理，是否返回空 -->
- 兼容性：<!-- 配置格式、命令、键位是否变化 -->

## 验证

- [ ] `cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings`
- [ ] `cargo test --manifest-path rust/Cargo.toml`
- [ ] `node scripts/smoke.js`
- [ ] `node test/harness.js /path/to/RuoYi-Vue`
- [ ] `npm run compile`

补充说明（新增断言、复现用例、基线数据等）：

```text

```
