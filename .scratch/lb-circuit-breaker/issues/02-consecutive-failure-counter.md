# 02: 连续失败计数与阈值禁用（含 max_consecutive_failures 设置项）

**What to build:** 网关自动熔断持续故障的供应商：某供应商的成员请求失败（所有失败，含不可重试 4xx）时内存计数 +1，任一成功请求清零；计数达到设置项 max_consecutive_failures（Int，默认 5，改后热生效）时，把该供应商连同名下全部虚拟模型条目一起停用、打上 failure_disabled 标记并记 warn 日志（含 request_id）。偶发失败不误伤，进程重启计数清零。

**Blocked by:** 01（禁用动作与 failure_disabled 标记就绪）

**Status:** ready-for-agent

- [ ] 内存连续失败计数（provider 粒度）挂入 AppState：失败 +1、成功清零、阈值比较逻辑有单元测试
- [ ] 设置项 max_consecutive_failures：种子默认 5、PUT 校验拒绝 0/负数/非整数、运行时热读取新值
- [ ] 转发链路所有失败路径（failover 4 分支 + 非可重试 4xx 终态）计数；4xx 不降级的现有行为不变
- [ ] 达阈值执行连续失败禁用：enable=false + 子模型级联停用（cascade_disabled 语义不变）+ failure_disabled=true + warn 日志；并发触达只禁用一次
- [ ] 集成测试：mock 上游连续失败达默认阈值 → 供应商与子模型全停用且带标记；中间夹一次成功 → 计数清零不触发；阈值改为 2 后两次失败即触发
