# 0004 — 前端数据面板与表单的模块收敛

## Status

accepted

## Context

数据面板概念（时间窗口 → 查询 → 图表/赛马）由五个 overview 页面手写复刻：每页各自实现窗口 map、subtitle、粒度推导与载入/错误/重试块（`windowSubtitle` 四页逐字节相同）；四张 RaceCard 各自复制 6 指标 COLUMNS 定义、排序翻转与深链 URL 拼接；八个数据 hook 各自复制 URLSearchParams 拼装、query key 与 keepPreviousData，窗口参数存在 ChartsParams/InsightParams/位置参数三种写法。每页测试需 mock 7-8 个 hook。另有供应商模型表单知识（zod schema、能力开关、proxy 校验）在三个弹窗各写两遍；纯格式化函数偷读 i18n 全局单例导致 en 分支不可测。

## Decision

按自底向上顺序分步实施，每步独立提交：

1. `useRaceSort` + `SortableMetricTable`（components/sortable-metric-table.tsx，同文件无依赖可直测）→ 数据层 → `RaceCardShell` → `StatsSection` 收拢五页骨架；页面保留逐区块 JSX 与查询调用，但每页只声明 `SECTION_KEYS` 区块配置，窗口 map、副标题、粒度与载入/错误/重试三态等重复骨架收进 `stats-section.tsx`（`useSectionWindows`/`useSectionSubtitle`/`SectionBody`/`StatsSection`/`CardStatsSection`）；四张赛马卡退化为「`useRaceCardWindow` 状态机 + 配置注入 `RaceCardShell` + `SortableMetricTable`」。默认窗口的三种工厂语义保持现状（只收拢代码不改行为）。
2. 数据层：新增共享窗口/过滤类型（替换三种参数写法），参数拼装、query key、keepPreviousData 收进内部 `statsQuery`；typed facade 保留原名，调用方零改动。
3. 表单：只收 zod schema（provider-models/provider-model-form.tsx 的 `makeProviderModelBaseSchema`）、4 能力开关网格（`CapabilitySwitchGrid` + `CAPABILITIES`）、proxy 校验规则三块（`proxySuperRefine` 与 `ProxyConfigFields` 同文件导出）；弹窗各自的打开/重置/toast 骨架不动。
4. locale：纯格式化函数加显式 `locale` 形参，组件经 `useTranslation` 在边缘传入；`api.ts` 的错误文案 `i18n.t` 不动。

被否决的备选：单页试点先行（收益延迟）；连弹窗骨架一起抽（弹窗间差异会逼出参数化泥潭）；formatter 工厂注入（一次性换掉所有调用点，面过大）。

## Consequences

- 窗口语义、排序翻转、深链拼装、参数序列化各有一处实现与一层直测；新增过滤维度从改 6 文件变改 1 处。
- 页面测试的 hook mock 泛滥收敛到模块级测试。
- en 格式化分支首次可单测，i18n 词条改动不再静默波及格式化函数。
- 分步推进期间新旧写法短暂并存，按 1→2→3→4 顺序合并后清理。
