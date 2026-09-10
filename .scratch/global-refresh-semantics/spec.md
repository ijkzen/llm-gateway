# SPEC — 顶栏刷新的「页面刷新」语义与页面刷新按钮下线

Status: implemented (2026-09-10)

来源：`.scratch/global-refresh-semantics/REQUIREMENTS.md`（2026-09-10 grilling 四轮 + ponytail 削减一轮，全部经用户拍板）。
实现：049a7d6（feat）+ 72ff197（docs）；审阅：code-review 两轴 + ponytail-review，修复项已落在 049a7d6。
决策记录：无新 ADR（改动易回滚、非难以逆转的取舍，按 domain-modeling 三测不写）。
术语：`CONTEXT.md` 已消歧——「远端模型刷新 (Model Refresh)」指拉取远端模型列表（原「刷新」词条更名），
新增「页面刷新 (Page Refresh)」指本次语义。

## Problem Statement

仓库里存在两层「刷新」，语义不统一且各有毛病：

- **顶栏全局刷新按钮**：调用 `queryClient.invalidateQueries()` 无参——只把缓存标记过期并重取当前活跃的查询，
  旧数据一直留在屏幕上，用户看不到「真的重新取了一次」；按钮本身没有任何反馈（不禁用、图标不转），
  点了不知道有没有生效。
- **5 个页面各自的刷新按钮**（供应商 / API Key / 定时任务 / 供应商模型 / 虚拟模型 页面头部）：
  只重取本页主查询，覆盖面还不一致——虚拟模型页渲染依赖三个查询，它的刷新按钮却只刷其中一个，
  切到另一页或换一个过滤条件/时间窗，缓存还在，看到的可能是旧数据。

用户希望只有**一个**刷新入口（顶栏），语义简单直白：**把这个页面的数据全部清掉重取一次**。

## Solution

1. **删除 5 个页面工具栏里的刷新按钮**，刷新统一由顶栏承担；各页原有的错误态「重试」保留。
2. **顶栏刷新按钮改为「页面刷新」**：点击后清空除登录态（`auth/me`）与版本号（`health`）之外的
   **全部**前端查询缓存，并立即重新取数当前页面。点击后当前页数据先清空（短暂骨架屏）再呈现新数据；
   其它页面、其它过滤条件/时间窗口的缓存也一并清掉，切页时重新请求（用户已确认接受该代价，
   换取「零维护、不可能漏登记」）。
3. **按钮加「正在刷新」反馈**：重取期间按钮禁用、图标旋转。

登录态与版本号被排除的原因：清 `auth/me` 会让路由守卫全屏显示「验证中」，且该请求 `retry: false`，
一次网络抖动就会把用户踢回登录页；`health` 只是侧栏版本号，重取无意义。

## User Stories

1. 作为管理后台使用者，我想在任何页面只看一个刷新按钮，以便不必记住「哪个页面该点哪个刷新」。
2. 作为管理后台使用者，我点顶栏刷新后，希望当前页的数据真的被丢弃并重新取一次，以便确认看到的不是旧缓存。
3. 作为管理后台使用者，我希望刷新过程中按钮禁用并转圈，以便知道刷新正在进行、不会重复点击。
4. 作为管理后台使用者，我希望刷新不会让我退出登录，以便刷新是安全无副作用的操作。
5. 作为管理后台使用者，我在请求日志页改了过滤条件后点刷新，希望**所有**过滤组合/时间窗的缓存都被清掉，
   以便重置条件或换回旧条件时看到的也是新数据，而不是陈旧的缓存。
6. 作为管理后台使用者，我在数据面板点刷新，希望当前时间窗与之前切过的其它时间窗都重新取数，同理。
7. 作为管理后台使用者，我在供应商页刷新后切到 API Key 页，接受该页也重新请求一次
   （缓存已被清），换取「刷新一定彻底」的确定性。
8. 作为管理后台使用者，当某个页面加载失败时，我仍能用页面上的「重试」按钮恢复，以便局部失败不必整页刷新。
9. 作为管理后台使用者，我在供应商详情看到用量卡时，「刷新用量」仍能强制向上游真实抓取（绕服务端缓存），
   因为它与「页面刷新」是不同的语义，不应被合并。
10. 作为管理后台使用者，我在「添加供应商模型」弹窗里仍能「尝试刷新」拉远端模型列表，以便导入新模型。
11. 作为管理后台使用者，定时任务日志弹窗 SSE 断线重连耗尽时，仍能整页重载恢复；应用崩溃时错误边界仍提供重载。
12. 作为维护者，我希望不新增「路由 → 查询键」的登记表，以便新增页面/查询时刷新不会静默少刷一部分。
13. 作为维护者，我希望刷新行为有测试锁定（清了什么、没清什么、是否重取、按钮状态），
   以便日后调整 predicate 时不会静默改错范围。
14. 作为维护者，我希望删除的页面按钮在页面测试里被断言不存在，以免被误加回来。

## Implementation Decisions

**新增一个小组件承载顶栏刷新按钮**（按钮 + 刷新逻辑同文件，只有一个消费方，不抽多余 hook 文件）：

- 刷新动作：`queryClient.resetQueries({ predicate })`，predicate 排除顶层键 `auth` 与 `health`
  （即 `queryKey[0] !== "auth" && queryKey[0] !== "health"`），其余全部命中。
- 选 `resetQueries` 而非 `removeQueries`/`invalidateQueries` 的原因（源码已核，
  `@tanstack/react-query@5.101.0` / `query-core`）：
  - `resetQueries` 先对全部匹配项（active + inactive）执行 `reset()` **清空数据**，
    再在同一批次内对 active 查询立即重取（`refetchQueries({ type: "active" })`，忽略 `staleTime`）
    ——这正是「删除数据 + 重新取一次」的语义。
  - `removeQueries` 对 active 查询不触发重取（observer 不订阅 QueryCache，须等下一次 render），
    且 `removeQueries` 之后再 `refetchQueries` 同键是**空操作**（条目已不在缓存），故排除。
  - `invalidateQueries` 不删数据，用户要的是清空语义，故排除。
- 前缀/匹配：`resetQueries` 默认递归前缀匹配；采用 predicate 全量筛选，无需任何键清单与路由映射。
- **重取中状态**：用 `useIsFetching({ predicate })`（同一 predicate）判断，`> 0` 即视为刷新中；
  按钮此时 `disabled`、`RefreshCw` 图标加 `animate-spin`。
  用同一 predicate 而非无参 `useIsFetching()`，避免登录态/版本号的后台请求误触发转圈。
- 按钮的可访问名与 title 沿用现有 i18n 键 `common.refresh`（中文「刷新」），不新增文案键。
- `layout.tsx` 中原来的内联按钮块（`Button` + `RefreshCw`）替换为该组件；`RefreshCw` 的 import
  若不再使用则一并清理。

**删除 5 个页面工具栏刷新按钮**，逐页删除 `PageHeader` 内的刷新 `Button` 及其不再使用的
`RefreshCw` import（`virtual-models` 页还须清理随之失去唯一引用的 `refetch` 变量——若 `refetch`
仍被错误态使用则保留；`provider-models` 页的 `refetchProviders`/`refetchModels` 同理）。
**不动**各页的 ErrorState `onRetry`。

**保留不动的刷新类控件**（语义不同，不得顺手合并）：
- 用量卡「刷新用量」（`providers.refreshUsage`）：递增 token 带 `?refresh=1`，绕服务端缓存真取上游。
- 添加供应商模型弹窗「尝试刷新」（`providerModels.tryRefresh`）：拉远端模型列表候选。
- 各错误态「重试」：ErrorState、race-card-shell、RequestLogsTable。
- 两处整页重载：`CronJobLogsDialog` 的 SSE 断线恢复「刷新」（`window.location.reload()`）、
  `ErrorBoundary` 的崩溃兜底「刷新页面」。
- 请求日志表格的「重置」：本地过滤状态重置，非缓存刷新。

**不新增**：无后端改动、无 API 契约、无设置项、无 schema 变更；不改全局 `staleTime`/`retry`；
不引入 toast/历史记录等附加交互；不建路由↔查询键注册表。

**文档**：`CONTEXT.md` 消歧已在 Stage 1 完成（「刷新」→「远端模型刷新」+ 新增「页面刷新」），
随本次 docs 提交；无 ADR。

## Testing Decisions

好测试的标准：只断言外部可观察行为（缓存内容、是否发起请求、按钮的可交互状态），
不测实现细节（不断言调用了哪个 queryClient 方法、不查内部 state）。

1. **刷新行为测试（主缝，新增一个测试文件）**——用真实 `QueryClient`（不 mock react-query）
   渲染新组件并配一个探针组件（`useQuery` 挂载一个查询以模拟「当前页 active 查询」），预置缓存后点击按钮：
   - 非全局的 inactive 缓存数据被清空（`getQueryData` 变 `undefined`）；
   - active 查询被重新取数（queryFn 调用次数增加，且新数据上屏）；
   - `["auth","me"]` 与 `["health"]` 的数据**原样保留**、queryFn 未被再次调用；
   - 重取进行中按钮 `disabled`，图标带旋转类；完成后恢复可点击。
   先例：`web/src/components/__tests__/provider-usage-card.test.tsx`（受控 queryFn + fireEvent），
   `web/src/test/setup.ts` 已有 localStorage / ResizeObserver / matchMedia 桩。
2. **5 个页面测试各加一条断言**：刷新按钮不存在（`queryByRole("button", { name: "刷新" })` 为 null），
   锁定删除不被回退。先例：现有 5 个 page 测试（`web/src/__tests__/{providers,api-keys,cron-jobs,provider-models,virtual-models}-page.test.tsx`）。
   注意这些测试只 mock 了 hook 的 `refetch`、从不点击刷新按钮，删除按钮**不需要**改 mock，
   仅需确认没有文案/快照断言依赖它。
3. **不改**：用量卡、添加弹窗、ErrorState、race-card、request-logs、cron-job-logs 的既有测试
   （对应控件行为不变）。

验证命令（提交门禁）：`cd web && pnpm lint && pnpm vitest run`；
后端未改动，仍跑 `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`
以符合仓库全量门禁约定。

## Out of Scope

- 「路由 → 查询键前缀」注册表及其防漏登记测试（经 ponytail 削减后由「清全部非全局」取代）。
- 后端/接口任何改动（含为页面刷新引入 `?refresh=1` 类参数）。
- 用量卡「刷新用量」、弹窗「尝试刷新」、各类「重试」、两处整页重载的语义合并或删除。
- 全局 `staleTime`/`retry` 默认值调整；请求日志的「重置」行为。
- toast 提示、刷新历史、快捷键、下拉菜单等未要求的交互。

## Further Notes

- **点击后的视觉**：当前页数据被清空（`data` 变 `undefined`），页面会短暂显示骨架屏/加载态，
  这是选定语义的一部分，用户已明确接受。
- **跨页代价**：在 A 页刷新会清掉 B 页缓存，切到 B 页时重新请求（可感知为「切换后加载一下」）。
  单用户后台收益大于代价；若未来觉得浪费，可退回「只清 active + 排除全局」的更懒版本（一行改动）。
- **单次刷新的请求量**：等于当前页挂载的查询数（其它页面的 inactive 查询只清不取，
  下次进入该页才请求），不会产生额外网络风暴。
- `["api-keys", id]` 查询带 `gcTime: 0`（明文 Key 不留存），reset 对它的影响与其他键一致，
  无特殊处理。
