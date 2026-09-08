# 01: 成员尝试核心——chat/native 双 failover 循环合一 + 空候选 503（C1）

**What to build:** `forward_chat` 与 `forward_native` 各 ~200 行的「逐个尝试成员」循环（解密 → 构建请求 → 上游调用 → 状态分类 → record_failure → 降级/重试 → 终态）收成唯一的成员尝试核心。chat/native 只注入两个 adapter：请求构建（已有 `build_upstream_call` / `build_native_upstream_call`）与错误信封整形（`openai_error` / endpoint 原生错误形状）；成功响应整形（`dispatch_success` / `dispatch_native_success`）留在调用方。排序仍由调用方做（`order_members` 在核心外）。**活 bug 修复**：native 路径空候选（全被额度剔除，`ordered[0]` panic）与 chat 一致返回 503 终态（按端点协议错误信封整形）。`forward_chat_direct` 单成员路径如能复用核心的「单次尝试」原语则复用，语义零变化。空候选/失败分类/重试序成为核心的纯决策，脱离 HTTP 可单测。

**Blocked by:** None（proxy/mod.rs 最热区域先行）。

**Status:** ready-for-agent

- [ ] `forward_native`（/v1/messages、/v1/responses）成员全被额度剔除时返回 503（不再 panic 500），错误信封按端点协议整形；chat 路径行为不变
- [ ] chat 与 native 双端在既有 failover 集成场景（tests/proxy_integration.rs 972-1185）逐项行为一致，不改写语义
- [ ] 核心新增单元测试：空候选 / 全耗尽 / 部分成员可重试 / 终态失败 的状态分类矩阵（当前循环内逻辑零单测）
- [ ] 降级消息与 LB 决策日志（format_usage 等）输出与现状一致（默认 info 级别可观测性不退化）
- [ ] 全量质量门绿（fmt / clippy -D warnings / cargo test / pnpm lint / vitest）

## Comments
