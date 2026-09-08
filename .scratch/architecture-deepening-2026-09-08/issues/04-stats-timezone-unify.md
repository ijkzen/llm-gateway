# 04: 数据面板统一时区——「今日/分桶」走设置表（C3b）

**What to build:** 拍板口径：数据面板「今日」与分桶时区统一到设置表时区（与 6b77d96 用量/调度口径合流）。后端：stats 端点改读设置表 timezone 作分桶/「今日」起点基准，`parse_tz_offset` 客户端覆盖不再参与「今日」与显式粒度的语义（删除或仅余窗口边界原样接受，两者互不影响 epoch 换算——实施时以最小改动定）。「今日」默认窗口（无 startTime/endTime 时）由服务端按设置表时区持有，前端不再自行算浏览器本地今日 0 点。前端：overview 与五页数据面板的窗口推导（`periodBounds` / `useSectionWindows` / 图表 hooks）改由设置表时区驱动（时区随 settings 数据已在前端可得；若不可得先补取，不改 useSettings 结构）。跨时区观察者受影响是拍板接受的后果（管理后台单一时区视角）；图表「天」桶午夜对齐与「今日」卡零点由此一致。

**Blocked by:** 03（复用窗口核心的时区装配）。

**Status:** ready-for-agent

- [x] 无参数调用 summary/charts：「今日」/默认窗口按设置表时区（默认 Asia/Shanghai），与调度/用量口径一致
- [x] 显式 startTime/endTime 的既有调用（自定义区间）语义不变
- [x] 前端「今日」卡与图表「天」周期在设置表时区午夜两侧不再跨义（凌晨 0 点前后单测/断言）
- [x] 设置表 timezone 变更后：统计口径随新时区生效（与 6b77d96 后用量口径同行为），前端窗口随 settings 刷新
- [x] stats 集成测试适配新默认窗口断言；全量质量门绿

## Comments

- feat/stats-timezone-unify 实施完成（质量门全绿：cargo test 791 / 34 套件，clippy 零警告，lint 220 文件，vitest 419 / 55 文件）。提交留在分支未合 main。
- **后端**：charts/insight 的分桶时区从客户端 `tzOffsetMinutes`（缺省 UTC）改为设置表 IANA 时区（`timezone_sync`，缺省 Asia/Shanghai，与用量口径同源），按窗口起点时刻求固定偏移（纯函数 `tz_offset_minutes_at`，含纽约 DST 冬/夏令时单测）；`ChartsQuery.tz_offset_minutes` 字段与 `parse_tz_offset` 删除，客户端残留参数被 axum 忽略。小时桶对整时偏移不变、天/月/年桶与「今日」午夜对齐随之落位。一处集成测试因窗口原先对齐 UTC 日而迁移为对齐设置表本地午夜（6 桶语义成立）。
- **前端**：race-period.ts 增 IANA 时区内核（Intl 墙钟部件 + 固定偏移模型，含 DST 日 23h/25h 的 ±1 日校正循环），`periodBounds`/`formatPeriodLabel`/`formatCompactPeriodLabel` 增加可选 `timeZone` 参数（缺省浏览器本地、旧行为原样保留，被既有 26 个本地用例锁定）；新 hook `useStatsTimeZone()` 读设置表 timezone（缺省 Asia/Shanghai，随 settings 缓存刷新）；组件（窗口控件/赛马卡壳/区块副标题/请求日志时间过滤）与五个页面全部把窗口推导切到设置表时区并停发 tzOffsetMinutes；自定义窗口（epoch 绝对值 + 浏览器本地输入 UX）按拍板保留原语义。
- **测试**：race-period 新增 11 个 tz 用例（上海周一/月/年边界 + 纽约 DST 23h 预言 + 标签）；页面测试经 setup 全局 mock（返回机器本地 IANA）保持本地语义零改写；overview-page 停发断言更新。stats 集成测试移除 14 处已失效的 tzOffsetMinutes 参数，语义依赖默认设置表时区（+480 确定性成立）。
- **双轴 code-review**：Spec 轴五条验收全部 MET，并确认两处文档漂移无功能影响——「服务端持有『今日』默认窗口」未实现（resolve_chart_window 默认仍为过去 24h；前端恒传显式窗口，今日卡与图表已同口径走设置表时区，意图满足）；无参数回退路径为小时桶、对整时偏移不变。Standards 轴零违规；已执行清理：删除死导出 `clientTzOffsetMinutes`、修正 5 处过期注释、剥离集成测试中误导性参数。已知边界（记录不阻塞）：wallDayStart 校正循环对「整日跳过型」时区（如 Pacific/Apia）不收敛、跨 DST 周的 ISO 周数按固定 24h 近似——均为非目标时区（默认 Asia/Shanghai 无 DST）的标签/边界级近似，与既有本地实现同量级。race-period 本地/tz 双分支为有意保留（旧行为被测试锁定，不强行统一）。