# 0022 — 取数窗口：窗口身份与取值分离（绝对时间不进 query key）

## Status

accepted

## Context

数据面板与请求日志的时间窗由「窗口定义」（周期 + 偏移 / 自定义区间）经 `raceWindowBounds(state, now, tz)` 解析成绝对起止 `{startTime, endTime}`，而 `now` 取组件挂载时刻并固化（`useState(() => Date.now())`），理由是让「当前周期」的窗口终点稳定、避免渲染抖动引发重复请求。

问题出在这个解析结果的去向：绝对起止被直接写进 react-query 的 `queryKey` 与请求参数（`use-request-logs.ts` 把整个 `filters` 当 key；8 个 stats hook 把 `window.startTime/endTime` 放进 `statsKey`）。于是**窗口身份携带了取值**，而 `now` 又被固化，导致：

1. **所有重取路径复用旧窗口**。顶栏刷新按钮走 `resetQueries`——用同一个 key 重跑 `queryFn`，拿到的仍是挂载时刻解析出的终点；窗口聚焦重取、失败重试同理。表现为：打开页面之后产生的请求日志行/统计数据，刷新多少次都看不到，必须重新加载页面或点页内「重置」。
2. **跨零点窗口整体停在昨天**。`startTime`（今日 0 点）同样由固化的 `now` 推出，过夜的标签页窗口不滚动。
3. `useRequestLogs` 此前只能靠「重置按钮里 `setNow(Date.now())`」局部打补丁，且该补丁只覆盖重置这一条路径。

## Decision

1. **窗口身份与取值分离**：新增 `QueryWindow { key; resolve() }`（`lib/race-period.ts`）。`key` 只含窗口定义（`period`/`offset`/自定义起止）与设置表时区——跨渲染、跨时刻恒定；`resolve()` 每次按**调用时刻**解析绝对起止。`queryWindow(state, tz)` 为构造入口。
2. **绝对起止只在取数时产生**：`statsQuery` 新增可选 `window` 选项，把 `window.key` 追加到 query key、在 `queryFn` 内调用 `resolve()` 拼进请求参数；端点不传 `window` 即不带时间参数（全历史累计）。9 个 stats hook（4×rank / 3×metrics / charts / insight）与 `useRequestLogs` 全部改为收 `QueryWindow`，参数里不再出现 `startTime`/`endTime`。
3. **一次修复覆盖全部重取路径**：手动刷新、窗口聚焦重取、失败重试、重新挂载都经由同一个 `queryFn`，因此都拿到调用时刻的窗口——不再为某条路径单独打补丁。
4. **窗口数学归位到 lib**：`RaceWindowState` 与 `raceWindowBounds` 从 `components/race-window-control.tsx` 迁至 `lib/race-period.ts`（组件层转出以保持导入路径），使 hooks 不再依赖组件层；`RaceWindow`/`TimeWindowParams` 两个因本次改造失效的类型删除。
5. **展示用 `now` 仍可固化**：窗口控件的标题/标签继续用挂载时刻的 `now`（不参与取数）。跨零点时数据正确、标签可能显示前一日——作为已知残留记录，不阻塞本次修复。
6. **深链窗口参数取定义值**：`raceHref` 不再接收解析后的绝对起止，自定义窗口直接取 `appliedCustom`（本身稳定），预设周期只带 `period`/`offset`。

## Consequences

- 「刷新即可见最新数据」成为取数层的结构性保证，而非某个按钮的行为；请求日志与数据面板各页一致。
- 跨零点自动滚窗（起点与终点随当前时刻重算）。
- query key 在时间维度上稳定，缓存复用语义正确（同一窗口定义的旧数据可安全保留为 `placeholderData`）；新增约束：时区必须进 key，否则设置表切时区不会触发重取。
- 取数窗口的窗口定义进 key、取值延后，使 `queryFn` 成为唯一需要感知「当前时刻」的位置——组件层渲染不再产生时间相关 key。
- 请求日志页此前那条「重置刷新 now」的补丁及其测试删除，由取数层语义取代。

规格：无独立 spec（缺陷驱动：请求日志页刷新取不到新行的排查与修复）。
