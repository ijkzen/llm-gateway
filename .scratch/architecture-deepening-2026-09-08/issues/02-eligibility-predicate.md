# 02: 可用性读侧谓词——「此刻能否参与选路」单一来源（C2）

**What to build:** availability 模块补只读谓词（组合：成员 enable ∧ provider enable ∧ disabled_reason=None ∧ 用量可用），用量判定调 `usage/types` 的 `subscription_usable` / `balance_usable`（原位不动，纯数据方法已单测）。影响选路的 4 个消费点全量切换：`order_members` retain 过滤（proxy/mod.rs 265-370，删除「同口径 as apply_usage_gate」注释型对表）、`forward_chat_direct` 裸查（~1470 行）、`failure_recovery::probe_gate`（142-163 三态再编码收敛）、`apply_usage_gate` 的用量不可用判定（persist.rs 171-190，动作本身与停用/恢复语义不动）。`usage_rank.rs` 保留 FEFO 纯比较器，但其内部重复表述（has_quota/worst_window 的「任一窗口为 0」逐窗判定 vs types 全局 `subscription_usable`）改为调用单一规范谓词，两口径一致性加单测锁定。口径本身（查不到余额的按量成员不剔除、窗口不可用判平、5h→周→月链）**零变化**——本轮只收实现，不收语义。实施时在 CONTEXT.md 可用性域补「选路可用 (Traffic-Eligible)」术语。

**Blocked by:** 01（同文件顺序实施：先收尝试循环，再切谓词，避免 proxy/mod.rs 双重构互相干扰）。

**Status:** ready-for-agent

- [x] 谓词矩阵单测：enable × disabled_reason 四态 × 用量可用/不可用/查不到（订阅已提供窗口为 0 → 不可用；按量余额为 0 → 不可用；查不到 → 可用）的交叉
- [x] usage_rank 逐窗判定与 types 全局谓词对同一窗口矩阵给出一致结论（一致性测试，防止口径漂移复现）
- [x] 4 个消费点切换后行为不变：额度门控停用/恢复、失败复查、FEFO 剔除顺序的既有集成测试全绿
- [x] CONTEXT.md「选路可用」术语落盘
- [x] 全量质量门绿

## Comments

- feat/eligibility-predicate 实施完成（质量门全绿：cargo test 785 / 32 套件，clippy 零警告，lint 219 文件，vitest 415）。提交留在分支未合 main。
- **调研修正工单前提**：数据层口径在本票开工前已收敛——`UsageData::subscription_usable`/`balance_usable`/`usable_for_billing_mode` 是唯一来源，`order_members` retain、`apply_usage_gate`、`probe_gate` 三个消费点都已直接调用它们（probe_gate 用 `usable_for_billing_mode == Some(true)` 三态裁决，非再编码）。真实重复点只有两处：① usage_rank 私有「同类最差剩余」窗口扫描（has_quota/cmp_deadline/cmp_window 共用）；② 实体层 enable/reason 组合在 load_members 过滤（只查 enable）与 forward_chat_direct 门禁处手拼。
- **落地形态**：① `UsageData::worst_window(kind)` 成为扫描唯一实现，usage_rank 三个比较点全部委托；`subscription_usable` 也改建其上（available 但无法推导的窗口对判定中立，语义与旧全窗口扫描逐项等价，含 used/limit 全缺的边界），两口径由矩阵测试锁定（含多池耗尽与 underivable 边界 fixture）。② availability 新增读侧谓词 `traffic_available`（启用 ∧ 无停用原因），替换 load_members、forward_chat_direct 与 enable_manual 幂等检查三处手拼；实体层与用量层分层各司其职（成员 enable 过滤仍在 load_members SQL，用量判定仍按 10 分钟缓存走 order_members retain）——组合语义不新造单谓词，避免把缓存新鲜度问题焊进实体谓词。
- 双轴 code-review：Standards 轴零违规（nit：测试内 QuotaWindow 全限定路径可读性）；Spec 轴指出「文档声称判定与排序共用同一扫描」在原实现下不成立（subscription_usable 仍全窗口扫描）——已按指摘把 subscription_usable 改建 worst_window 之上，声明成真并补 underivable 边界测试；「组合谓词未一次成型」与「has_quota 仍保留逐层判定」两项为分层设计的刻意保留（逐层判定是 FEFO 排序的问题域，全局 usable 是剔除的问题域），差异由一致性测试锁死。
- CONTEXT.md 可用性域补「选路可用 (Traffic-Eligible)」术语（分层定义 + _Avoid_）。