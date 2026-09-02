# 03: 前端积分池徽标

**What to build:** 用量卡片中带窗口标签的窗口行显示池名徽标，管理员能一眼区分通用池与专属池的用量；无标签的窗口（既有所有厂商）展示完全不变；窗口行 key 不撞。

**Blocked by:** 01: SenseNova fetcher 全链路

**Status:** done

- [x] 窗口行有 label 时显示池名徽标，无 label 时展示不变
- [x] 窗口列表 key 按 window+label 去重，多池不撞键
- [x] 前端测试（先例：provider-usage-card 测试）覆盖徽标与多池渲染
- [x] 前端 lint/vitest 全绿
