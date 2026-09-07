# 03: 门控 / LB / 展示零改动验证

**What to build:** 确认 SiliconFlow 产出的余额型用量自动接入现有「余额耗尽自动停用/恢复」门控、按量 LB 排序与前端用量卡片展示，且这些共享逻辑零改动——通过测试证明 fetcher 输出（primary=合计）在这些既有接缝上行为正确。

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] 用 01 的归一化输出（primary=合计的「账户余额」行）核对：`UsageData::balance_usable()` → primary>0 可用 / =0 不可用，路径与 DeepSeek/OpenRouter 等按量供应商一致
- [ ] 核对 `apply_usage_gate` 按量分支对 SiliconFlow 供应商自动生效（无需新增分支）
- [ ] 核对 LB 按量排序 `primary_balance()` 取合计金额
- [ ] 核对前端 ProviderUsageCard balance grid 逐条渲染（账户余额行 + 券明细行）
- [ ] 上述以现有 persist/usage_rank/前端测试佐证；如发现 SiliconFlow 归一化输出与共享谓词预期不符，回到 01 修正并补单测
