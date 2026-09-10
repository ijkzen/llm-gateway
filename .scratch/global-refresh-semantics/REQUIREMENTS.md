# REQUIREMENTS — 顶栏刷新语义改造与页面刷新按钮下线

## 请求原文

> 删除每个页面自己的刷新按钮，并更新页面顶部工具栏刷新按钮的语义：把这个页面所有缓存的数据全部删除，然后重新刷新一次该页面。

## 现状（排查结论，2026-09-10）

两层刷新按钮，语义不统一：

- **全局顶栏按钮**（`web/src/components/layout.tsx:159-167`）：`queryClient.invalidateQueries()` 无参——
  失效全部查询并重取 active 的；不删数据、无反馈（不禁用、图标不转）。
- **页面工具栏按钮**（挂在 `PageHeader` 右侧插槽）：5 个页面各一个，只 `refetch()` 本页查询：
  - `providers.tsx:67` → `useProviders()`
  - `api-keys.tsx:44` → `useApiKeys()`
  - `cron-jobs.tsx:61` → `useCronJobs()`
  - `provider-models.tsx:215` → `useProviders()` + `useProviderModels()`
  - `virtual-models.tsx:206` → `useVirtualModels()`（**只此一个**，同页还渲染 `useProviders`/`useProviderModels`
    喂弹窗选择器——覆盖面缺口）
- 其余页面（overview、settings、request-logs、chat、四个详情页）没有页面级刷新按钮。

## 本次范围（已拍板）

1. **删除 5 个页面工具栏刷新按钮**（providers / api-keys / cron-jobs / provider-models / virtual-models）。
   顺带消除虚拟模型页「刷新只覆盖三个查询中的一个」的不一致。
2. **顶栏刷新按钮语义改为「页面刷新」**：清空缓存 + 重新取数，点击后当前页数据先清空
   （短暂骨架屏）再重取。用户确认接受该视觉。
3. **排除布局级键**：`["auth","me"]`（清了触发 RequireAuth 全屏「验证中」，且该请求 `retry:false`
   ——网络抖动会把用户踢回登录页）与 `["health"]`（仅版本号）不清。`["settings"]` 不排除
   （设置页主数据；其它页刷新会连带多一次小请求，可接受）。
4. **清理范围 = 清全部非全局**（grilling 中先选「只清本页」，经 ponytail 削减后改判）：
   `resetQueries({ predicate })` 一次清掉除上述两个键外的所有缓存——含其它页面、其它参数组合
   （请求日志别的过滤条件、数据面板别的时间窗等）。代价：在 A 页刷新会连带清掉 B 页缓存，
   切页时重新请求（单用户后台可忽略）。
5. **按钮加「重取中」反馈**：重取期间禁用 + 图标旋转。
6. **保留**：用量卡「刷新用量」（`providers.refreshUsage`，绕服务端缓存真取上游）、
   添加模型弹窗「尝试刷新」（`providerModels.tryRefresh`，拉远端模型列表）、各错误态「重试」
   （ErrorState / race-card-shell / RequestLogsTable）、两处整页重载按钮
   （`CronJobLogsDialog` 的 SSE 断线恢复、`ErrorBoundary` 的崩溃兜底）。

## 非目标（ponytail 削减后明确不做）

- **不建「路由 → 查询键」注册表**。原方案需手写 13 条路由的键前缀清单（4 个详情路由 `NAV_GROUPS`
  未覆盖），是手写副本、有漂移风险；改「清全部非全局」后零注册表、零维护、不可能漏登记。
- 不改任何后端接口、不加服务端参数（不引入 `?refresh=1` 类语义——那是用量卡已有的能力）。
- 不引入刷新历史、toast 提示、快捷键等附加交互。
- 不改 `staleTime` / 全局 QueryClient 默认值。
- 不改请求日志表格的「重置」（本地过滤状态重置，非缓存刷新）。

## 关键机制（已核实，实现须遵守）

- 实际版本 `@tanstack/react-query@5.101.0`（`web/node_modules`，package.json 声明 `^5.60.0`）。
- **`resetQueries({ predicate })` 是唯一匹配语义的原语**（源码 `query-core/queryClient.js:126-140`）：
  先对全部匹配项（active + inactive）`reset()` 清空数据，再在同一 batch 内对 active
  `refetchQueries({type:"active"})` 立即重取、**忽略 staleTime**；返回 `Promise<void>`。
- `removeQueries` 被排除：对 active 不触发重取（observer 不订阅 QueryCache），
  且 `removeQueries` 后再 `refetchQueries` 同键是**空操作**（条目已不在缓存）。
- `invalidateQueries` 被排除：不删数据（用户要的是「删除」语义与清空闪烁）。
- 全局默认 `staleTime: 5min`、`retry: 1`（`web/src/main.tsx:14-21`）。
- 前缀匹配是递归前缀：`["providers"]` 会命中 `["providers",1]`；因改为清全部非全局，
  无需精细前缀管理。

## 需同步的文档

- `CONTEXT.md`：现有「**刷新 (Refresh)**」词条语义是远端模型列表拉取，与本次第二个「刷新」撞名。
  已消歧（本次 Stage 1 完成）：
  - 词条更名为「**远端模型刷新 (Model Refresh)**」，`_Avoid_` 加「刷新（裸称，与页面刷新混淆）」；
    「候选模型」词条内引用的「刷新返回的」同步改为「远端模型刷新返回的」。
  - 新增「**页面刷新 (Page Refresh)**」词条（前端域），描述清全部非布局级缓存 + 重取当前页。
- **不写 ADR**（按 domain-modeling 三测：改动易回滚、一个函数；不满足「hard to reverse」）。
- `AGENTS.md` 无相关描述，无需改。

## 验收口径（供 spec/工单展开）

- 5 个页面工具栏不再有刷新按钮；各页原有 ErrorState 重试仍可用。
- 任一页面点顶栏刷新：当前页数据清空 → 重取 → 呈现新数据；按钮期间禁用且旋转。
- 刷新不会把用户踢回登录页（`auth/me` 未被清）；版本号（health）不被重取。
- 刷新后切到其它页面：该页缓存也已被清、会重新请求（预期行为，非缺陷）。
- 保留的刷新类控件（用量卡、尝试刷新、重试、两处整页重载）行为完全不变。

## 来源

- 排查：本会话（2026-09-10）前端代码通读 + 两个只读子代理事实核查。
- 拍板：用户四轮 AskUserQuestion（删除范围 / 删除语义 / 清理范围 / 全局键 / 反馈 / 重载按钮 / 削减提案）。
