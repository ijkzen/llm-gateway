# 01: 成员尝试核心——chat/native 双 failover 循环合一 + 空候选 503（C1）

**What to build:** `forward_chat` 与 `forward_native` 各 ~200 行的「逐个尝试成员」循环（解密 → 构建请求 → 上游调用 → 状态分类 → record_failure → 降级/重试 → 终态）收成唯一的成员尝试核心。chat/native 只注入两个 adapter：请求构建（已有 `build_upstream_call` / `build_native_upstream_call`）与错误信封整形（`openai_error` / endpoint 原生错误形状）；成功响应整形（`dispatch_success` / `dispatch_native_success`）留在调用方。排序仍由调用方做（`order_members` 在核心外）。**活 bug 修复**：native 路径空候选（全被额度剔除，`ordered[0]` panic）与 chat 一致返回 503 终态（按端点协议错误信封整形）。`forward_chat_direct` 单成员路径如能复用核心的「单次尝试」原语则复用，语义零变化。空候选/失败分类/重试序成为核心的纯决策，脱离 HTTP 可单测。

**Blocked by:** None（proxy/mod.rs 最热区域先行）。

**Status:** ready-for-agent

- [x] `forward_native`（/v1/messages、/v1/responses）成员全被额度剔除时返回 503（不再 panic 500），错误信封按端点协议整形；chat 路径行为不变
- [x] chat 与 native 双端在既有 failover 集成场景（tests/proxy_integration.rs 972-1185）逐项行为一致，不改写语义
- [x] 核心新增单元测试：空候选 / 全耗尽 / 部分成员可重试 / 终态失败 的状态分类矩阵（当前循环内逻辑零单测）
- [x] 降级消息与 LB 决策日志（format_usage 等）输出与现状一致（默认 info 级别可观测性不退化）
- [x] 全量质量门绿（fmt / clippy -D warnings / cargo test / pnpm lint / vitest）

## Comments

- feat/member-attempt-core 实施完成（质量门全绿：cargo test 780 通过 / 31 套件，clippy -D warnings 零警告，pnpm lint 219 文件，vitest 415）。`forward_through_members` + `ForwardFlavor`（Chat/Native）+ `FailureStage` 落地；`MemberLoopOutcome::Succeeded(AttemptSuccess)` 把成功上下文交还调用方分派（dispatch_* 函数体未动）；排序仍在调用方。
- 空候选 503 语义统一进核心；native 路径由 panic 修复为按端点协议信封的 503（`native_messages_quota_exhausted_returns_503_instead_of_panic` 回归测试 + chat 侧 `chat_quota_exhausted_returns_503` 双端实证）。native 空候选错误文案此前不可观察（panic），现与 chat 同文案（订阅制额度均已耗尽）。
- 有意行为变更（已在提交信息说明）：① 失败行 `RequestRecord.stream` 统一记 `client_stream`——chat 旧循环错误行恒记 false 是全库唯一偏离（native/direct/dispatch 各臂均记尝试类型），对齐后数据面板流式数对失败行口径一致；② native 降级路径新增与 chat 同款 warn 日志（原先静默），信息级日志无变化；③ `record_failure` 删除全调用点未用的 `_client_stream` 死参数（chat 侧历史行为注释见 ①）。
- code-review（双轴）发现均已处理或显式接受：成功分派移回调用方（review 指出与工单「留在调用方」文字不符）；`RequestFlags::default()` 替代手写默认。接受项：循环内四段失败分支仍同构（保留显式 warn 文案，抽 helper 需闭包早退改造，收益不抵复杂度）；FailureStage 矩阵单测覆盖分类、空候选与信封由双端集成测试覆盖。提交留在 feat/member-attempt-core 分支未合 main。