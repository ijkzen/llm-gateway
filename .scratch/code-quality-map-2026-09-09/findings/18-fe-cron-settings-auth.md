# FINDINGS · 18 FE 任务/设置域审查（2026-09-10）

范围：cron-jobs 域（pages/cron-jobs.tsx + 5 组件 + use-cron-jobs/use-cron-job-logs）+ settings 域（pages/settings.tsx/not-found.tsx + 7 组件 + use-settings）+ 全部相关 `__tests__` 盘点；SSE 后端契约交叉核对（08 票结论引用）。方法：两个子代理分域全读 + 主代理对全部 P2 与关键 P3 行号磁盘复核。清单模式：不改代码。login/auth/chat 已拆 21 票不在此范围。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 18-01 | P2【已修复 2026-09-10】 | 契约/SSE | 后端实时 `log` 事件从不带 seq（log_capture.rs:151 写死 None+skip 序列化），前端 `data.seq<=last.seq` 去重**失效**（undefined 比较恒 false=不丢弃）→ 快照重叠必现重复行；实时日志 React key 恒 undefined；测试 emitLog 注入 seq 构成假阳性契约——08-01 前端半边实证 |
| 18-02 | P2【已修复 2026-09-10】 | 逻辑/竞态 | `reset` 分支 fetchQuery 整体替换 logs：reset 与拉取完成间追加的实时日志被覆盖丢弃；叠加全局 staleTime 5min，fetchQuery 可能命中旧缓存把新日志**回退** |
| 18-03 | P2【已修复 2026-09-10】 | 逻辑/i18n | 设置表直接编辑 `language` 只改后端：前端 i18n 不热切换、刷新后仍读旧 localStorage，前后端语言长期分叉无提示（正路 useChangeLocale 只有 locale-toggle 在用） |
| 18-04 | P2【已修复 2026-09-10】 | 逻辑/契约 | SettingEditDialog 类型盲：非 Json 一律纯文本 `z.string()`+Input，Int/Bool/Float 无字段级校验与对应控件（max_consecutive_failures 首当其冲），非法值只能提交后吃后端 400 |
| 18-05 | P3 | 逻辑 | 断线重连错过 run_ended 时 idle 分支不 invalidate runs，历史执行列表长期停留旧快照 |
| 18-06 | P3 | 逻辑/契约 | cron 表达式前端仅 `.min(1)` 非空（语法校验后端独有且仅变更时），非法表达式只得泛化 toast 无字段级错误 |
| 18-07 | P3 | 逻辑 | 立即执行后固定 1s 失效刷新，但 last_run_at 在执行**结束**才回写——超过 1 秒的任务刷新不到新时间 |
| 18-08 | P3 | 逻辑/一致性 | CronJobDeleteDialog 的 onError 也关弹窗，与 Provider/ApiKey/Setting/VirtualModel 删除弹窗「失败保持打开」不一致 |
| 18-09 | P3 | 性能 | 实时日志列表无 memo/虚拟化，每条新日志整表重渲染+逐行重算 formatDateTime，2000 条上限下累计 O(n²)；18-01 的 key 冲突进一步退化 diff |
| 18-10 | P3 | 测试覆盖 | cron 域缺口族：reset/run_started/onerror/Lagged/seq 去重分支全未测；cron-jobs 页只验选中态（弹窗全被 mock 成桩）；List/Detail/Edit/Delete 四组件零测试；两 hooks 零直测 |
| 18-11 | P3 | 健壮/性能 | SSE onerror 恒置 reconnecting 无退避无终止：会话过期 401 时浏览器无限重连、界面永久「重连中」 |
| 18-12 | P3 | UX | SettingEditDialog 不展示被编辑的 key/type（Json 弹窗有 key），上下文缺失 |
| 18-13 | P3 | 逻辑 | Json 结构化编辑静默丢数据：数组丢空串条目、对象丢空键行、重复键后者覆盖前者（现有测试反而固化丢弃行为） |
| 18-14 | P3 | 状态 | ImportDialog 成功/关闭后 file 状态不复位、file input 未清空：重开显示旧文件且确认可用，可重复误导入 |
| 18-15 | P3 | 契约 | 改密 zod 按 JS 字符串长度、后端按 UTF-8 字节：非 ASCII 密码前端放行后端 400，文案「characters」误导 |
| 18-16 | P3 | 测试覆盖 | settings 域缺口族：settings.tsx 页/SettingEditDialog/use-settings/not-found 均无测试（01 盘点两项确认成立）；Json raw 模式改动后保存、改密长度边界、导入残留（18-14）无覆盖 |
| 18-17 | P3 | 简洁 | 三弹窗「mutation+toast+onOpenChange」样板同构可抽共享 helper（use-settings 冻结，宜放域内工具） |
| 18-18 | P3 | 简洁 | SettingsTable 的 pageIndex 重置 effect 与 TanStack 默认 autoResetPageIndex 重叠且依赖不含 searchQuery/typeFilter——注释所述职责实际由框架承担，冗余 |
| 18-19 | P3 | 观察 | 类型筛选下拉只有 5 个已知类型，后端可能返回的 `Unknown` 行不可筛选（badge 有兜底） |
| 18-20 | P3 | 观察 | 设置删除无前端保护键禁用（language/timezone 照常展示删除菜单，靠后端 400 兜底；11-09 同族） |

**本票无需拍板项**。i18n 硬编码锚点汇总见末节（归 20 票施工）。

## 各条证据

### 18-01 SSE log 无 seq：去重失效+key 冲突（P2，08-01 前端半边）

后端：log_capture.rs:151 `seq: None` + :42-43 `skip_serializing_if` → 实时 log 事件序列化后无 seq 键；落库 seq 由 worker 生成（worker.rs:441-455），快照有 seq。前端：use-cron-job-logs.ts:133-135 `if (last && data.seq <= last.seq) return s`——`undefined <= number` 为 false → **永不丢弃**（失效而非误伤：不少报、只重复）；:138-141 追加项 seq=undefined → CronJobLogsDialog.tsx:205-207 `key={log.seq}` 恒 undefined。后端注释（cron_jobs.rs:328/335-336）明确把去重责任推给前端 seq——契约断裂。测试 cron-job-logs-dialog.test.tsx:139 的 emitLog 显式注入 seq 与真实契约不符（假阳性）。默认解=08-01 拍板（全链路补 seq）落地后自愈；过渡期前端 seq 改可选 + key 换稳定回退。波及面：useCronJobLogStream 唯一消费=CronJobLogsDialog；CronJobLog.seq 声明必填 number 会误导，应改可选。

### 18-02 reset 分支整体替换竞态（P2）

use-cron-job-logs.ts:174-190：reset 后 fetchQuery 拉全量替换 logs（:185 `{...s, logs}`）。①reset→resolve 间经 log 分支追加的行被覆盖（广播不重放，只存 DB——历史展开可见=缓解）；②fetchQuery 受全局 staleTime 5min：该 run 日志若 5 分钟内被历史区拉取过，直接返回旧缓存不发请求 → reset 把已显示的新日志回退成旧快照。③落库攒批（worker.rs LOG_BATCH_SIZE=50/~100ms flush）天然滞后实时行。默认解：替换改合并（按 seq/本地索引去重），或该查询 staleTime=0+refetchQueries。注：stateRef.current=state（:92）渲染期赋值属反模式，本例可用但脆弱。

### 18-03 设置表改 language 不热切换（P2，i18n）

use-settings.ts:35 更新成功只 invalidate settingsKeys.all；前端语言由 zustand 持久化 store 驱动（use-locale.ts:17-49，初始读 localStorage）。正路 useChangeLocale（:59-76：切 i18n+PUT+全量失效）只被 locale-toggle.tsx:23 使用。settings.tsx 对 language 行走通用 updateSetting → 后端同步 cron 标题并落库，前端 UI 不变、刷新仍读旧 localStorage，长期分叉无提示。默认解：settings.tsx 对 language 行改走 useChangeLocale（调用侧修，不动冻结的 use-settings）。timezone 无此问题（useStatsTimeZone 经失效即时读新值）。

### 18-04 SettingEditDialog 类型盲（P2，契约）

SettingEditDialog.tsx:25-27 zod 仅 `z.string()`、:94 恒纯 Input；settings.tsx:63 只把 Json 分流到 Json 弹窗。后端 settings.rs:66-94：Int 须可解析 i64、Float f64、Bool 精确 true/false（不 trim=11-08）；max_consecutive_failures 另要 ≥1（14-03）。前端零字段级校验/控件（Bool 无 select、Int 无 number input）。默认解：按声明类型渲染控件+zod 同口径校验。

### 18-05 重连 idle 不刷新历史列表（P3）

use-cron-job-logs.ts:119-121（idle）/:105-117（snapshot）均不 invalidate runs；只有 run_started/run_ended（:154/:171）失效。run 在断线窗口内结束 → 重连收 idle → 历史列表停留旧数据（无 refetchInterval+staleTime 5min）。默认解：snapshot/idle 分支一并 invalidate runs。

### 18-06 表达式前端零校验（P3，契约）

CronJobEditDialog.tsx:30 仅 min(1)；后端 cron_jobs.rs:143-163 表达式变更才解析校验。非法表达式只得泛化 toast。默认解：前端轻量校验或把 400 msg 映射到字段。

### 18-07 立即执行 1s 失效拿不到新时间（P3）

use-cron-jobs.ts:50-55 setTimeout(1000) 失效；last_run_at 在执行结束才回写（scheduler.rs:721-744）。任务 >1s 时刷新读到旧值。默认解：run_ended 事件联动失效或详情手动刷新。

### 18-08 删除失败关弹窗（P3，一致性）

CronJobDeleteDialog.tsx:24-27 onError 同时 onOpenChange(false)；Provider/ApiKey/Setting/VirtualModel 删除弹窗均失败不关。默认解：onError 只 toast。

### 18-09 实时日志 O(n²) 渲染（P3，性能）

CronJobLogsDialog.tsx:205-207 整表 map 无 memo/虚拟化；LogLine（:41-53）每渲染重算 formatDateTime。2000 条上限下逐条追加累计 O(n²)。默认解：React.memo(LogLine)+预格式化 ts。

### 18-10 cron 域测试缺口（P3）

已锁：snapshot+log 追加/run_ended 截断/idle 空态/历史展开/滚动跟随/卸载关闭/选中态。缺口：reset/run_started/onerror/Lagged/seq 去重/切任务重置全未测；cron-jobs 页弹窗全 mock 成桩无接线断言；List/Detail/Edit/Delete 零测试；两 hooks 零直测。

### 18-11 SSE 无退避无终止（P3，健壮）

use-cron-job-logs.ts:102-103 onerror 恒 reconnecting；EventSource 无限重连。会话过期 401 时永久「重连中」。默认解：readyState/错误判定后停止并提示。

### 18-12 设置弹窗缺上下文（P3，UX）

SettingEditDialog.tsx:78-99 全程不出现 key/type（JsonSettingEditDialog.tsx:115 有 key）。默认解：头部补 key+类型 badge。

### 18-13 Json 结构化静默丢数据（P3）

JsonSettingEditDialog.tsx:85 数组丢空串、:87-91 丢空键行/重复键覆盖；测试 json-setting-edit-dialog.test.tsx:89-107 反而固化丢弃。当前唯一 Json 种子（allowlist）不允许空条目故无实害。默认解：保存前校验提示。

### 18-14 ImportDialog 文件残留（P3，状态）

ImportDialog.tsx:52 file state 成功/关闭不复位（BackupDialog 常驻挂载）；:134-143 file input 未清空（重选同文件不触发 onChange）。默认解：成功/关闭 setFile(null)+清 input.value。

### 18-15 改密长度口径（P3，契约）

ChangePasswordDialog.tsx:29 zod 按 UTF-16 code unit；后端 auth.rs:68 按字节。128 汉字（384 字节）前端放行后端 400，文案「characters」误导。默认解：前端按字节校验。注：改密后保留当前会话是后端正确语义，前端不跳转无问题。

### 18-16 settings 域测试缺口（P3）

01 盘点两项确认：settings.tsx 页零测试（Json/非 Json 分支路由、loading/error、四弹窗编排）；SettingEditDialog 无专属测试（18-04/18-12 无回归网）。另：use-settings 零直测、Json raw 模式改动后保存、改密长度边界、导入残留（18-14）、BackupDialog 导出文件名无断言。

### 18-17 弹窗 mutation 样板（P3，简洁）

SettingEditDialog.tsx:64-75/JsonSettingEditDialog.tsx:96-107/SettingDeleteDialog.tsx:18-26 同构。默认解：域内共享 helper。

### 18-18 pageIndex effect 冗余（P3，简洁）

SettingsTable.tsx:198-202 依赖 [settings] 不含 searchQuery/typeFilter；TanStack autoResetPageIndex（非 manualPagination 默认真）已承担注释所述职责。默认解：删 effect。

### 18-19 Unknown 类型不可筛选（P3，观察）

SettingsTable.tsx:221-225 筛选项=SETTING_TYPES 五值；后端 Unknown 行 badge 有兜底（:64-69）筛选项没有。观察级。

### 18-20 删除保护键无前端禁用（P3，观察）

SettingDeleteDialog 无保护判断，language/timezone 删除菜单照常（SettingsTable.tsx:169-175），靠后端 400（11-09 同族）。默认解：前端对保护键禁用删除入口（可选）。

## i18n 硬编码锚点汇总（归 20 票施工）

- SettingsTable.tsx:99/101/107/109/128/130/139/141/148/159/168/174/233-234
- SettingEditDialog.tsx:69/72/82/92/103/106
- JsonSettingEditDialog.tsx:101/104/114/127/143/150/159/168/180/185/188
- ChangePasswordDialog.tsx:28-34/65/68/78/79/89/102/115/126/129
- SettingDeleteDialog.tsx:21/24/33/34-39/40
- 关键佐证：zh-CN.ts:545-564 与 en.ts 对应段**已有** settings.* 键（key/value/type/updatedAt/edit/editValue/oldPassword/newPassword/passwordMismatch/changeSuccess 等），组件未用——20 票可直接复用既有键。
- 已干净：settings.tsx、not-found.tsx、BackupDialog.tsx、ImportDialog.tsx 全走 t()。
- 17 票域内实例：ProviderUsageCard.tsx:55/64/202、ProviderModelSection.tsx:69-72（已记 17-16/17-21）。

## 已核验无问题区（避免后续票重复审查）

- **SSE 契约其余面**：事件名 snapshot/idle/log/run_started/run_ended/reset 前后端一致；log 分支 run_id 归属过滤与 run_ended 归属校验正确；once(initial).chain(updates) 保证快照先于增量；cleanup es.close() 有测试锁定。
- **弹窗固定头底**：CronJobLogsDialog（固定头+内层双滚动区）/CronJobEditDialog/删除弹窗走 ConfirmDialog 全合规，本域无新增违规实例。
- **CRUD 字段同步**：CronJob/CronJobLog/CronJobRun 与后端 JobResponse/LogResponse/RunResponse 逐字段一致。
- **改密流程**：后端吊销其他会话保留当前（auth.rs:222/254-259），前端成功仅关弹窗不跳转=正确。
- **备份链路**：ApiError msg 贯通后端中文错误正确展示；导入成功全量失效；确认文案「未包含的设置项保留」与后端 upsert 语义一致。
- **parseJsonMode**：嵌套/非字符串/null/非法 JSON 回退原文正确。
- **时区变更前端即时性**：失效→useStatsTimeZone 即读新值。
- **not-found.tsx** 全 i18n 无硬编码。

## 性能/内存轮结论

无 P1/P2 性能项。动作项=18-09（日志列表 memo/预格式化）与 18-11（重连退避）；18-02 的 staleTime 叠加既影响正确性也削弱「重拉」语义；设置表规模极小无性能面；Json 编辑器大 JSON 每击键整弹窗重渲染为观察级（当前设置规模无风险）；SSE 每弹窗 1 连接无压力。结论：本域性能形态健康，重点是 18-01/18-02 两个 SSE 正确性项随 08-01 批实施。

## 实施进度（2026-09-10）

- **18-01 已修复**（随后端 08-01）：`JobLogLayer` 捕获侧按 span 分配 per-run 单调 seq，实时 log 事件携带；前端既有 `data.seq <= last.seq` 去重生效、React key 不再是 undefined。前端新增去重回归（快照 1/2 + 重叠实时事件各只渲染一条），`emitLog` 不再注入模拟 seq。
- **18-02 已修复**：reset 分支改为「按 seq 合并」而非整体替换（拉取期间新到的实时日志不丢），并 `staleTime: 0` 绕过旧缓存回退。
- **18-03 已修复**：`useUpdateSetting` 在 key=language 的成功回调里热切换前端语言（zustand store + i18n.changeLanguage + 全量失效缓存），设置表直视编辑不再与前端分叉。
- **18-04 已修复**：`SettingEditDialog` 按声明类型渲染控件与校验——Bool 用 Switch、Int/Float 带 inputMode 与 zod refine（与后端 validate_setting_value 同口径），非法值当场标错不再提交后吃 400；补 `settings.validationInt/Float/Bool` 双语词条。
