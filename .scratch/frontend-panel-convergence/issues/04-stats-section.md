# 04: StatsSection：五页窗口统计骨架收敛

**What to build:** 五个 overview 页面退化为区块配置表：窗口 map、副标题本地化、粒度推导、载入/错误/重试块收进一个窗口统计区块模块（interface：过滤维度 + 区块类型 + 窗口状态）。页面测试的 hook mock 泛滥消退——窗口切换→参数、错误→重试在区块层验证一次。

**Blocked by:** 02（区块经统一数据层取数）、03（赛马区块复用卡壳）。

**Status:** ready-for-agent

- [x] 区块层验证：窗口变更产生正确查询参数、错误态重试重发正确查询、载入态展示
- [x] 五页换用区块配置后视觉与交互不变（副标题、默认窗口、深链）
- [x] 页面内逐字节重复的副标题/骨架副本删净
- [x] 至少一个页面测试改写为经区块验证，mock 面明显收窄
- [x] pnpm lint + vitest 全绿

## Comments

- be18f08（feat/frontend-panel-convergence）实施完成。useSectionWindows + useSectionSubtitle/sectionGranularity + StatsSection/CardStatsSection 收拢五页骨架；页面净减约 300 行；页面测试仅补 useSearchParams mock，352 全绿。
