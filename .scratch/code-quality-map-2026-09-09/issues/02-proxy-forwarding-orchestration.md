# 02 · proxy 转发编排与选路审查

Type: task
Status: resolved
Blocked by: 01

## Question

对 proxy 转发编排与选路域做全量审查：`lb.rs` / `route.rs` / `calls.rs` / `headers.rs` / `forward.rs`（含 forward_chat_direct）/ `failover.rs` / `native.rs` / `usage_rank.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：空候选守卫、failover 边界、选路/排序口径、头组装顺序、并发竞态等潜在 bug；
- 实现简洁：重复分支、可收敛拷贝（2026-09-08 已收口成员循环/路由解析/统一流泵的**残余**，不复查已整改项本身）；
- 测试覆盖：哪些分支无测试锁定（对照 48 场景矩阵等已有测试）；
- 模块间调用：与 usage 缓存/额度门控、availability 谓词、cron failure_recovery 探活、metrics 落库的调用是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/02-proxy-forwarding-orchestration.md`，Answer 给摘要与需拍板问题。

## Answer

审查完成（2026-09-09）：全范围文件（lb/route/calls/headers/forward/failover/native/usage_rank/failure_recheck/mod + 域内 tests）逐行通读 + 跨域接口抽查（dispatch record_failure/sse_response、entity virtual_model、/v1/models 过滤口径）。产物 = `findings/02-proxy-forwarding-orchestration.md`：**12 条分级清单（0 P1 / 1 P2 / 11 P3）+ 已核验无问题区**。

核心结论摘要：

- **P2（02-01）**：成员终态失败零 tracing 日志——降级分支每条 warn，终态（末成员或重试关闭）分支只落 request 表直接 return；全文件唯一 error 级日志在不可达兜底里。建议终态补与降级同形状日志。
- **三项拍板已记录**：02-07 额度空候选 503 落库记失败行（NoMembers 分支顺带补 warn——它原来连日志都没有）；02-08 直连成功清零行为保持现状只补注释（场景化后用户定：成功=健康信号为有意设计）；02-06 决策日志明细两条降 debug、info 只留选路结果。
- **逻辑正确性**：空候选两路径守卫齐全（无 C1 时代 panic 残留）；降级行 request_id-N 后缀与终态行区分正确；同一 provider 每请求一次失败计数；原生透传接口门+协议防御过滤+读体失败 502 口径与 chat 一致；headers 四层组装与 spec 逐条相符（域内单测厚）。
- **测试轴**：usage_rank 比较器 22 单测 + 与门控一致性回归质量高；缺口=build_native_upstream_call 零单测、failover 终态行为无单测。
- 其余 P3：calls.rs 头组装尾段 20 行逐字重复、Gemini failover 图片重复下载、resolve_usage_map DB 错误静默→全量真实抓取、裸数字常量、F10 i18n 锚点登记等。

实施建议优先级：02-01（可观测性）> 02-07/02-06（已拍板，随实施批）> 其余 P3。
