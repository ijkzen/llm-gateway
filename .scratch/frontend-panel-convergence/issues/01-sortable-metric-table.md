# 01: useRaceSort + SortableMetricTable：赛马排序与列定义收敛

**What to build:** 四张赛马卡与两页内嵌赛马表共用一套排序 hook 与可排序指标表：六指标列定义（标签/格式化/默认排序方向）与排序翻转逻辑各只有一份（与指标汇总卡已有的指标定义合并），深链 URL 拼接收为纯函数。各卡保留自己的差异部分（数据 hook、行点击路由、标题图标）。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [x] 排序 hook：默认降序、点同列翻转、点新列重置——直测
- [x] 深链 URL 纯函数：自定义窗口带起止时间、预设窗口带周期与偏移——直测
- [x] 四卡两页全部换用共享实现，各自旧副本（列定义/排序/URL 拼接）本票删净
- [x] 渲染测试：点表头断言排序与深链（先例：图表组件测试）
- [x] pnpm lint + vitest 全绿

## Comments

- 1a7a05d（feat/frontend-panel-convergence）实施完成。RACE_COLUMNS 六指标列定义 + useRaceSort + 泛型 SortableMetricTable 收拢四卡两页；windowQueryString 深链纯函数与 initialWindowFromUrl 往返有测试；排序翻转/新列默认方向直测。

- 评审整改：删除 useRaceSort/SortableMetricTable 的投机 columns 参数与 RACE_COLUMN_LABEL_KEYS（全部调用点均用默认列集），interface 收窄。
