# 03: RaceCardShell：赛马卡壳层收敛

**What to build:** 四张赛马卡的壳层——窗口控件、滚动懒加载、载入骨架/错误重试/无数据分支——收为一个卡壳组件，每张卡退化为约几十行配置（数据 hook、行点击、标题）。默认窗口的三种工厂语义保持现状，只收拢代码不改行为。

**Blocked by:** 01（表格与排序来自共享实现）、02（数据来自统一数据层）。

**Status:** ready-for-agent

- [x] 四卡行为与升级前一致：懒加载时机、错误重试、无数据态、深链跳转
- [x] 壳层状态分支有一处渲染验证
- [x] 每卡旧壳层副本删净
- [x] pnpm lint + vitest 全绿

## Comments

- 44a57e6（feat/frontend-panel-convergence）实施完成。RaceCardShell + useRaceCardWindow：图标/标题/副标题/窗口控件卡头与懒加载/骨架/错误重试/无数据分支单处实现；四卡各约 60 行配置；分支渲染测试 5 例。
