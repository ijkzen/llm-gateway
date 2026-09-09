# 12 · 鉴权与中间件域审查

Type: task
Status: open
Blocked by: 01

## Question

对鉴权与安全域做全量审查：`auth/` 模块（argon2/session 表/中间件/Bearer）、`routes/auth.rs`、`middleware/`（CORS/Trace/CatchPanic）、api_key 鉴权与 `crypto` 的加密边界及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：会话 7 天/改密踢会话、Bearer 查找与 key_hash、初始化流程竞态、CORS 已知风险现状、panic 捕获遗漏面；
- 实现简洁：中间件与路由的重复校验、错误响应映射；
- 测试覆盖：auth_integration 之外缺什么（时序竞态、边界 token）；
- 模块间调用：auth 与 routes/proxy/entity 的依赖方向是否合适（领域逻辑是否泄漏进中间件）。

产出 `.scratch/code-quality-map-2026-09-09/findings/12-auth-middleware-domain.md`，Answer 给摘要与需拍板问题。
