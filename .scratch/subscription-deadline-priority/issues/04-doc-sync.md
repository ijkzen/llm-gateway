# 04: 注释与文档同步

**What to build:** 排序语义描述同步为新算法：usage_rank 模块顶部注释、proxy/mod.rs 排序相关注释、「5h→周→月 剩余百分比逐层、同层打平比重置时间」的描述 → 「逐层额度检查 → 更上层截止时间链优先 → 剩余百分比兜底」。lb_48_scenarios.rs 文件头补一句「订阅测试数据无截止时间，走剩余百分比兜底路径」。AGENTS.md 虚拟模型排序描述同步更新。

**Blocked by:** 01（订阅制比较器重写 + 单测）

**Status:** ready-for-agent

- [ ] usage_rank.rs 模块注释反映新算法
- [ ] src/proxy/mod.rs 排序语义注释同步
- [ ] tests/lb_48_scenarios.rs 文件头注释补充兜底路径说明
- [ ] AGENTS.md 用量感知排序描述同步（描述新算法而非贴代码）
