# FINDINGS · 19 FE 共享组件与基础设施域审查（2026-09-10）

范围：`components/ui/`（chart/sidebar/dialog/form/select 等重点全读）+ `components/data-table/` 全族 + 通用组件（mid-ellipsis/confirm-dialog/skip-to-main/theme-toggle/empty-state/error-boundary/multi-select/locale-toggle/layout）+ `lib/`（api.ts/utils/constants/pages/backup + stats-query/race-types 接线面）+ `types/` + `App.tsx`/`main.tsx`/`test/setup.ts` + `vite.config.ts`。方法：两个子代理分域全读（组件族 / lib+装配），全部 P2 与关键 P3 经主代理磁盘复核（含 ky 1.14.3 node_modules 源码实证）。清单模式：不改代码。17 票的 DialogScrollShell 定案归本域（ui/ 层）实施批落地，本票不重复展开。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 19-01 | P2【已修复 2026-09-10】 | 逻辑 | sidebar 折叠状态写 cookie 但全仓无任何读取点——刷新必回展开，注释声明的持久化不成立（shadcn SSR 遗产在 Vite SPA 无读者） |
| 19-02 | P2【已修复 2026-09-10】 | 逻辑/网络栈 | `beforeError` 把 HTTPError 换成 ApiError（name 不匹配），ky 重试的 `isHTTPError` 判不上 → `retry.statusCodes` 白名单失效：GET 类 400/401/403/404 也被重试一次，429/503 的 Retry-After 尊重逻辑同被绕过 |
| 19-03 | P2【已修复 2026-09-10】 | 逻辑/网络栈 | 网络错误/超时根本不过 `beforeError`（ky 只在 !response.ok 后调），`api.ts:53` 的 NETWORK_ERROR 分支不可达——网络故障 toast 显示英文原始消息 |
| 19-04 | P2【已修复 2026-09-10】 | 逻辑/超时 | 全局 `timeout: 10000` 短于用量上游 15s：usage/estimate/backup import 未像 refresh(30s)/test(60s) 那样覆盖——前端先于后端超时，后端其实会成功落缓存 |
| 19-05 | P3【保持现状（用户拍板 2026-09-10）】 | 性能 | MidEllipsis 每实例挂载 O(log n) 次「写 DOM→读 offsetWidth」强制同步重排，46 处实例（表格按行）叠加无共享测量无缓存 |
| 19-06 | P3【已修复 2026-09-10】 | 逻辑 | MidEllipsis 容器 clientWidth=0（隐藏页签/首帧未布局）时把任何非空文本渲染成「…」，靠 ResizeObserver 自愈 |
| 19-07 | P3【已修复 2026-09-10】 | 健壮 | multi-select 硬编码 DOM id（select-all/选项值），RequestLogsTable 一页 4 实例重复 id（当前靠弹层互斥掩盖） |
| 19-08 | P3【已修复 2026-09-10】 | 逻辑/潜在 | chart.tsx tooltip `item.value &&` 吞 0 值（当前调用全传 formatter 不可达，属潜在缺陷） |
| 19-09 | P3【已修复 2026-09-10】 | 逻辑 | sidebar Ctrl/Cmd+B 无可编辑目标/event.repeat 守卫；useIsMobile 首帧恒 false 移动端闪一帧桌面侧栏 |
| 19-10 | P3 | i18n | error-boundary.tsx:29-30/36 硬编码中文（锚点确认归 20 票；类组件可用全局 i18n 单例直调）；另观察：hasError 不随路由重置，出错后只能整页刷新 |
| 19-11 | P3【已修复 2026-09-10】 | 测试覆盖 | `ui/` 与 `data-table/` 零测试目录；ErrorBoundary/LocaleToggle/DataTablePagination/ChartContainer/useSidebar/getPageNumbers 零测试引用 |
| 19-12 | P3【已修复 2026-09-10】 | 逻辑 | `/login` 懒加载但在 AppLayout 的 Suspense 之外——直达/硬刷新 /login 白屏等 chunk（无 boundary 不抛错只挂起） |
| 19-13 | P3【已修复 2026-09-10】 | 性能 | react-query `retry:1` × ky `retry:1` 叠加 → 失败查询最多 4 次 HTTP 尝试（与 19-02 叠加后 4xx 也打满） |
| 19-14 | P3【已修复 2026-09-10】 | 逻辑 | 全局 401 跳转 `location.assign("/login")` 丢失 RequireAuth 的 `from` 回跳——会话过期后登录回不到原页面 |
| 19-15 | P3【已修复 2026-09-10】 | 测试覆盖 | `lib/api.ts` 零测试：unwrap 两抛错分支/401 钩子三判定/beforeError 三分支/fetchHealth 特例全裸 |
| 19-16 | P3【已修复 2026-09-10】 | 逻辑 | `middleEllipsis` 守卫用 UTF-16 length、截断按码点：代理对文本返回值 .length 可超 maxLength（SVG 轴宽预算面窄） |
| 19-17 | P3【已修复 2026-09-10】 | 测试覆盖 | utils 缺口：getPageNumbers（off-by-one 风险五分支）/formatPercent/formatDateTime/cn/localeOf 无测试 |
| 19-18 | P3【已修复 2026-09-10】 | 模块间 | 时区键 `"timezone"` 三处独立定义（i18n/index、use-stats-time-zone、test/setup）+ 默认值两处；statsFilterKeySegments 重抄 StatsFilter 形状未复用 |
| 19-19 | P3【已修复 2026-09-10】 | 简洁 | `pages.ts` 的 `PAGES` 死代码（全仓只用 NAV_GROUPS）；`ApiError.statusCode` 无消费点（投机面） |
| 19-20 | P3【已修复 2026-09-10】 | i18n | `lib/utils.ts` 的 `formatDateTime` 硬编码 `toLocaleString("zh-CN")`（ProviderDetail/CronJobDetail 消费，EN 界面仍中文格式） |
| 19-21 | P3【已修复 2026-09-10】 | 性能 | vite manualChunks 的 `react-router-dom` 匹配不到 react-router 核心包——router chunk 实为薄壳，分包意图部分落空 |
| 19-22 | P3【已修复 2026-09-10】 | 测试基建 | setup.ts 全局 mock `use-stats-time-zone`：真实 hook（读设置表挑 timezone 行）零覆盖，mock 常量与真实模块双份字面量静默漂移面 |

**本票无需拍板项**。

## 各条证据

### 19-01 sidebar cookie 只写不读（P2）

ui/sidebar.tsx:20-21 定义 SIDEBAR_COOKIE_NAME、:85 写 cookie（注释「keep the sidebar state」）、:58 初值恒 `defaultOpen=true`；grep 全仓无读取点。折叠后刷新必回展开。shadcn 原版 cookie 是给 SSR 框架读的，SPA 无读者。默认解：初值处读 cookie 恢复，或删写入（二选一，实施批定）。

### 19-02 beforeError 破坏 ky 重试白名单（P2，网络栈）

api.ts:32-54 beforeError 对每个 HTTP 错误返回 `new ApiError(...) as unknown as HTTPError`（name="ApiError"）。ky 1.14.3 流程（node_modules 实证 Ky.js:58-67）：`!response.ok` → 先跑 beforeError 替换错误 → throw；retry 决策（Ky.js:255-290）先 `isHTTPError(error)`（instanceof 或 name 匹配，ApiError 均不满足）→ 判不上就跳过 `statusCodes` 白名单（默认 [408,413,429,500,502,503,504]）直接 `#calculateDelay()` 重试。后果：retryMethods 含 get/put/delete 等 → 确定性 4xx（400/401/403/404）也重试一次（默认退避 ~300ms）；429/503 的 Retry-After 尊重逻辑同被绕过；401 时 afterResponse 已触发跳转、重试还再打一次。默认解：beforeError 不换错误身份（改 message+挂 apiCode 字段），或显式 `retry.shouldRetry` 收口。

### 19-03 NETWORK_ERROR 分支不可达（P2，网络栈）

beforeError 只在 `!response.ok` 分支被 ky 调用（Ky.js:58-67），fetch 拒绝/超时从 `ky.#fetch()`/timeout 直接抛出不过钩子 → api.ts:53 的 `new ApiError(error.message, "NETWORK_ERROR")`（response 为空分支）永不执行。业务层 toastError 把 error.message 作 description → 网络故障时中文界面显示 `Failed to fetch`/`Request timed out: GET ...` 英文原文。默认解：调用侧统一 catch 按 TimeoutError/TypeError 映射本地化文案，或删分支注释明「仅 HTTP 错误」。

### 19-04 全局 10s 超时短于用量 15s（P2，超时）

api.ts:74 `timeout: 10000`；usage/http.rs:13 后端上游 15s；use-provider-usage.ts:43-46/use-usage-estimate.ts:30-33/lib/backup.ts:85-87 均无 timeout 覆盖（对照 use-provider-models.ts:193/:205 有 30s/60s 先例）。上游响应落 10-15s 时前端先失败（两 hook retry:false 不自愈），后端实际成功落缓存，重进才见。默认解：用量/预估/导入端点补 `{ timeout: 30000 }`。

### 19-05 MidEllipsis 强制同步重排（P3，性能）【保持现状 2026-09-10】

mid-ellipsis.tsx:44-53 二分循环每轮写 textContent+读 offsetWidth（强制 reflow，~6 轮/实例），46 处实例且表格按行渲染（ProviderList/ApiKeysTable/CronJobList/multi-select 选项等），单次挂载数百次重排。默认解：共享离屏测量节点按 (text,width) memo。

### 19-06 MidEllipsis 零宽渲染「…」（P3）【已修复 2026-09-10】

mid-ellipsis.tsx:26/28/47/56：available=0 时二分恒不满足 → best=0 → 渲染「…」（隐藏页签/首帧未布局；ResizeObserver 触发后自愈，恒 0 宽元素本就不可见）。默认解：`available <= 0` 时交回全文。

### 19-07 multi-select 硬编码 id（P3，健壮）【已修复 2026-09-10】

multi-select.tsx:135/139/166/170 硬编码 DOM id；RequestLogsTable.tsx:409/422/436/471 一页 4 实例 → id 重复，label htmlFor 解析到首个同名元素（Radix Popover 互斥卸载故未暴露）。默认解：useId() 前缀。

### 19-08 chart tooltip 吞 0（P3，潜在）【已修复 2026-09-10】

ui/chart.tsx:244 `item.value &&` → value=0 不渲染；当前调用全传 formatter（走 :199-200 分支）不可达。默认解：`item.value != null &&`。

### 19-09 sidebar 快捷键与首帧（P3）【已修复 2026-09-10】

sidebar.tsx:96-106 keydown 只判 key+修饰键（不判可编辑目标/event.repeat——输入框里 Ctrl+B 也切侧栏）；:68+use-mobile.tsx:6 首帧 undefined→false 恒桌面渲染。默认解：守卫可编辑目标与 repeat；useIsMobile 初值 matchMedia 同步求值。

### 19-10 error-boundary 硬编码中文（P3，i18n）

error-boundary.tsx:29-30/36（归 20 票；类组件可直调全局 i18n 单例，main.tsx:1 链已初始化）。附带观察：getDerivedStateFromError 只置 hasError 不随路由重置，出错后整页停留错误页至刷新（有刷新按钮兜底）。

### 19-11 ui/data-table 零测试（P3，测试覆盖）

ui/ 与 data-table/ 无 __tests__；ErrorBoundary/LocaleToggle/DataTablePagination/DataTableColumnHeader/DataTableViewOptions/ChartContainer/ChartStyle/useSidebar/getPageNumbers 零测试引用。默认解：优先 sidebar（toggle+cookie）、chart（ChartStyle 映射+tooltip）、DataTablePagination 三处。

### 19-12 /login 缺 Suspense 边界（P3）【已修复 2026-09-10】

App.tsx:23 lazy + :59 路由在受保护分支外；唯一 Suspense 在 layout.tsx:171-183 的 Outlet 外层。直达 /login → 无 boundary 挂起等 chunk（白屏，React 19 不抛错）。默认解：Routes 外包 Suspense 或登录页非懒加载。

### 19-13 双重 retry 叠加（P3，性能）【已修复 2026-09-10】

main.tsx:11-18 retry:1 × api.ts:75 retry:1 → 最多 4 次 HTTP 尝试（5xx/网络路径；ky 默认 retryOnTimeout=false 故超时不叠）。默认解：二选一收敛并在 main.tsx 注明分工。

### 19-14 401 跳转丢 from（P3）【已修复 2026-09-10】

api.ts:60-70 location.assign("/login") vs require-auth.tsx:27 的 `state={{from}}`——会话过期跳转无 from，登录后回不到原页。默认解：跳转前存 sessionStorage 或交 RequireAuth 统一处理。

### 19-15 api.ts 零测试（P3，测试覆盖）【已修复 2026-09-10】

unwrap 的 code!=="0"/data undefined 两分支、afterResponse 三判定（401/auth 白名单/登录页白名单）、beforeError 三路径、fetchHealth 特例全裸。默认解：lib/__tests__/api.test.ts 用 mock fetch 覆盖 + 19-02 回归。

### 19-16 middleEllipsis 口径（P3）【已修复 2026-09-10】

utils.ts:47-57 守卫 UTF-16 length、截断按码点——代理对文本返回长度超 maxLength（dashboard-charts.tsx:221/339 当字符预算用于 SVG 轴）。默认解：守卫改码点长度。

### 19-17 utils 测试缺口（P3，测试覆盖）【已修复 2026-09-10】

getPageNumbers（:157-187 五分支）/formatPercent/formatDateTime/cn/classNames/localeOf 无用例；已有 formatTokenCount/topWithOther/middleEllipsis 等。

### 19-18 契约常量重复定义（P3，模块间）【已修复 2026-09-10】

`"timezone"` 三处（i18n/index.ts:15、use-stats-time-zone.ts:4、test/setup.ts:73）+默认值两处；stats-query.ts:24-35 statsFilterKeySegments 重抄 race-types.ts:26-35 StatsFilter 形状。默认解：import 单源。

### 19-19 死代码（P3，简洁）【已修复 2026-09-10】

pages.ts:81-91 PAGES 零引用（layout 只用 NAV_GROUPS，两份顺序表漂移面）；ApiError.statusCode 无消费点。默认解：删。

### 19-20 formatDateTime 硬编码 zh-CN（P3，i18n）【已修复 2026-09-10】

utils.ts:139-144；消费方 ProviderDetail.tsx:238/241、CronJobDetail.tsx:87/93。同目录有 localeOf 未用。默认解：加 locale 参数（归 20 票一并）。

### 19-21 manualChunks 漏 react-router 核心（P3，性能）【已修复 2026-09-10】

vite.config.ts:31-38 `react-router-dom` 不匹配 `node_modules/react-router/`；router chunk 薄壳、核心落默认 chunk。默认解：改判 `node_modules/react-router`。

### 19-22 setup 全局 mock 掩盖真实 hook（P3，测试基建）【已修复 2026-09-10】

test/setup.ts:70-74 mock @/hooks/use-stats-time-zone 返回本机时区；真实逻辑（useSettings 挑 timezone 行+空值回退）零覆盖；mock 与真实模块两份常量字面量静默漂移面。默认解：局部 mock 或真实 hook 单测。

## 已核验无问题区（避免后续票重复审查）

- **依赖方向单向**：业务层零直用 Radix（grep 零命中）；recharts 图元直用属 shadcn 设计（chart.tsx 只封装 Container/Tooltip/Legend）。
- **chart 无 CSS 注入**：toPieConfig 只产 {label} 被 ChartStyle 过滤，dangerouslySetInnerHTML 只吃代码常量。
- **data-table 分页契约**：setPageSize 重算 pageIndex（table-core 8.21.3 实证）；getPageNumbers 分段正确。
- **confirm-dialog**：不碰 document.body；嵌套 pointer-events 有回归断言；默认标题非 cron 专属无误导。
- **theme**：三态完整有测试；index.html 首屏内联脚本防暗色闪白；use-theme hydrate 门控+view-transition 清理正确。
- **MidEllipsis 测量内核成立**：二分单调、RO 观察根节点、overflow-hidden 兜底（19-06 是唯一边界）。
- **constants 与后端枚举逐一对齐**（LB 0-3/降级 0-1/接口 0-4/SettingType 五值），未知值兜底 common.unknown。
- **NAV_GROUPS 与路由表逐条对齐**无孤儿；isPageActive 前缀点亮正确。
- **ky prefixUrl 用法合法**（调用点均无前导斜杠）；unwrap 契约与后端 Response 对齐（data undefined 分支不可达无害）。
- **setup.ts polyfill**：localStorage 语义完整+早于 i18n 导入；ResizeObserver/scrollIntoView 桩正确。
- **api 层固定开销小**：afterResponse 致每响应一次 clone+cancel，量级可忽略。

## 测试覆盖盘点

本域现状：components/__tests__ 32 个中本域仅 6 个（confirm-dialog/empty-state/mid-ellipsis/multi-select/skip-to-main/theme-toggle）；ui/+data-table/ 零测试；lib/api.ts 零测试；lib/utils 部分（19-17）；race-period 充分（~40 例含 DST）；stats-query 3 helper 有测；App 级装配（lazy/Suspense/RequireAuth 组合）无测试（19-12 因此未被抓）；main.tsx 无测试。缺口优先级：api.ts（19-15）> ui/data-table 三处（19-11）> App 装配（19-12）> utils 纯函数（19-17）。

## 性能/内存轮结论

动作项=19-04（超时不匹配，影响用量刷新成功率，最优先）、19-13（双 retry）、19-05（MidEllipsis 列表场景收敛）、19-21（分包修正）。其余面健康：api 单例无请求级分配、面板查询无轮询、lazy+Suspense 粒度合适（仅 /login 缺边界）、chart/sidebar 无重渲染热点。结论：本域主干健康，网络栈三兄弟（19-02/19-03/19-04）是唯一值得实施批优先的簇。

## 实施进度（2026-09-10）

- **19-06 已修复**：`MidEllipsis` 在容器 `clientWidth <= 0` 时直接展示全文，不再先渲染成「…」。
- **19-07 已修复**：`MultiSelect` 的 DOM id 加 `useId()` 前缀——同页多实例不再重复 id。
- **19-08 已修复**：图表 tooltip 的数值判断由真假值改为 `!= null`（0 也显示）。
- **19-09 已修复**：侧栏快捷键忽略 `event.repeat` 且跳过输入框/文本域/下拉/可编辑元素（输入框内 Ctrl+B 不再收起侧栏）。
- **19-11 已修复（关键缺口）**：新增 `sidebar.test.tsx`（默认展开、折叠写 cookie、cookie 恢复、三条快捷键守卫），并给 `test/setup.ts` 补 `matchMedia` 桩（此前 jsdom 缺该 API，响应式组件不可测）。
- **19-12 已修复**：`App.tsx` 用 `Suspense` 包住整棵路由树，直达/硬刷新 `/login` 不再白屏等 chunk。
- **19-13 已修复**：ky 层 `retry` 由 1 改 0，重试统一由 react-query 承担（此前叠加最多 4 次请求）。
- **19-14 已修复**：401 整页跳转前把来源路径写入 `sessionStorage`（新增 `AUTH_REDIRECT_FROM_KEY`/`readStoredRedirectFrom`，读取即消费），登录页优先取 router state、其次读暂存——会话过期后能回到原页面。
- **19-15 已修复**：`lib/__tests__/api.test.ts` 已覆盖 beforeError 两分支、错误身份保持、userErrorMessage 四分支、unwrap 两分支；本次补 401 来源路径的暂存/消费测试。
- **19-16 已修复**：`middleEllipsis` 的长度守卫改按码点（与截断口径一致），含代理对文本不再突破上限。
- **19-17 已修复**：新增 `lib/__tests__/utils.test.ts`——`getPageNumbers` 五分支与交界、`formatPercent`、`formatDateTime`、`localeOf`、`cn`、`middleEllipsis` 码点边界。
- **19-18 已修复**：`STATS_TIME_ZONE_KEY` 改为从 `i18n/index` 的 `SETTING_KEY_TIMEZONE` 派生，不再各处写字面量。
- **19-19 已修复**：删除零引用的 `PAGES` 常量与无消费点的 `ApiError.statusCode`。
- **19-20 已修复**：`formatDateTime` 增加 `locale` 参数，`ProviderDetail`/`CronJobDetail` 透传 `localeOf(i18n.language)`。
- **19-21 已修复**：vite `manualChunks` 的匹配改为 `node_modules/react-router`（原规则匹配不到核心包）。
- **19-22 已修复**：新增 `hooks/__tests__/use-stats-time-zone.test.tsx`，经 `importActual` 直测真实实现（读 timezone 行 / 缺行与空值回退默认 / 常量与后端一致）。
- **19-05 保持现状（用户拍板）**：共享离屏测量节点属收益有限、改动面较大的优化，不做；19-06 已修掉唯一的功能性边界。

- **19-02 已修复**：`beforeError` 不再替换错误身份——改为在 `HTTPError` 上覆盖 `message`（后端信封 msg）并挂 `apiCode` 字段；ky 的 `isHTTPError()` 判定恢复，`retry.statusCodes` 白名单与 Retry-After 尊重重新生效（确定性 4xx 不再重试）。
- **19-03 已修复**：新增 `userErrorMessage`（网络 TypeError → `error.networkError`、`TimeoutError` → 带方法/路径的 `error.timeout`、AbortError → `error.aborted`），`useToastActions.toastError` 统一经它取描述；两 locale 补 `error.timeout`/`error.aborted` 词条。
- **19-04 已修复**：全局 timeout 10s → 30s；用量查询与用量预估显式 `timeout: 30000`（后端上游 15s），备份导入 `timeout: 120000`。
- 测试：新增 `src/lib/__tests__/api.test.ts`（8 例：beforeError 两分支 + 错误身份保持、userErrorMessage 四分支、unwrap 两分支），`provider-detail.test.tsx` 补 17-01 竞态回归。前端 428 passed + tsc + biome 全绿。

- **19-01 已修复**：`sidebar.tsx` 增加 `readSidebarCookie()`，`_open` 初值改为 `readSidebarCookie() ?? defaultOpen`——折叠后刷新保持折叠（原先只写不读）。
