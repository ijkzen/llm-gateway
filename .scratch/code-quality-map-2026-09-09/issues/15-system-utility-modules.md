# 15 · 系统工具与基础单体域审查

Type: task
Status: open
Blocked by: 01

## Question

对系统工具与基础单体域做全量审查：`backup.rs` / `crypto/` / `config/` / `state.rs` / `i18n.rs` / `logs_cleanup.rs` / `response.rs` / `static_assets/` / `main.rs` / `lib.rs`（run 生命周期与 handler 注册）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：备份 JSON 解析/校验边界、i18n 占位符、优雅关闭时序、初始化失败路径、key 派生与加密格式版本；
- 实现简洁：各单文件内重复、跨文件的同构 helper（如时间/路径处理）；
- 测试覆盖：各模块单测之外缺什么（备份恢复演练、关闭竞态）；
- 模块间调用：handler 注册与 seed 双源一致性（08 票同查，此处查 lib.rs 侧）、state 聚合面是否合理。

产出 `.scratch/code-quality-map-2026-09-09/findings/15-system-utility-modules.md`，Answer 给摘要与需拍板问题。
