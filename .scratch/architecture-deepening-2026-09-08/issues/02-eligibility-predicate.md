# 02: 可用性读侧谓词——「此刻能否参与选路」单一来源（C2）

**What to build:** availability 模块补只读谓词（组合：成员 enable ∧ provider enable ∧ disabled_reason=None ∧ 用量可用），用量判定调 `usage/types` 的 `subscription_usable` / `balance_usable`（原位不动，纯数据方法已单测）。影响选路的 4 个消费点全量切换：`order_members` retain 过滤（proxy/mod.rs 265-370，删除「同口径 as apply_usage_gate」注释型对表）、`forward_chat_direct` 裸查（~1470 行）、`failure_recovery::probe_gate`（142-163 三态再编码收敛）、`apply_usage_gate` 的用量不可用判定（persist.rs 171-190，动作本身与停用/恢复语义不动）。`usage_rank.rs` 保留 FEFO 纯比较器，但其内部重复表述（has_quota/worst_window 的「任一窗口为 0」逐窗判定 vs types 全局 `subscription_usable`）改为调用单一规范谓词，两口径一致性加单测锁定。口径本身（查不到余额的按量成员不剔除、窗口不可用判平、5h→周→月链）**零变化**——本轮只收实现，不收语义。实施时在 CONTEXT.md 可用性域补「选路可用 (Traffic-Eligible)」术语。

**Blocked by:** 01（同文件顺序实施：先收尝试循环，再切谓词，避免 proxy/mod.rs 双重构互相干扰）。

**Status:** ready-for-agent

- [ ] 谓词矩阵单测：enable × disabled_reason 四态 × 用量可用/不可用/查不到（订阅已提供窗口为 0 → 不可用；按量余额为 0 → 不可用；查不到 → 可用）的交叉
- [ ] usage_rank 逐窗判定与 types 全局谓词对同一窗口矩阵给出一致结论（一致性测试，防止口径漂移复现）
- [ ] 4 个消费点切换后行为不变：额度门控停用/恢复、失败复查、FEFO 剔除顺序的既有集成测试全绿
- [ ] CONTEXT.md「选路可用」术语落盘
- [ ] 全量质量门绿

## Comments
