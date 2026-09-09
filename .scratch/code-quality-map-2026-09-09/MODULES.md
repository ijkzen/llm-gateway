# 模块边界全景盘点 — MODULES（01 票产出，2026-09-09）

范围：`src/`（后端）与 `web/src/`（前端）。方法：模块级 import 图脚本化全量统计（生产代码，内联 `#[cfg(test)]` 已剥除、文档注释引用已剔除）+ 子代理语义扫描 + 载重结论逐条对照磁盘复核。行数为文件总行数（含内联单测）近似值，仅表体量。

## 1. 后端模块图

### 1.1 模块清单与体量（src/，生产代码行数 ≈）

| 模块 | 生产 LOC | 依赖 | 职责 |
| --- | --- | --- | --- |
| entity | 649 | — | 14 张表 SeaORM 实体（最底层） |
| response / i18n / crypto | 86 / 50 / 138 | — | 统一信封 / 中英文案 / AES-GCM 加解密原语 |
| config | 77（+内联测试） | — | 环境变量配置 |
| db | 644（+内联测试 560） | entity | 连接/迁移（26 版） |
| app_settings | 272 | entity, i18n | 设置缓存（热生效） |
| middleware | 20 | app_settings, response | CORS/Trace/CatchPanic 三件套 |
| auth | 422（+79 内联） | response, app_settings, crypto, entity, i18n, state | 登录/会话/Bearer/拦截 |
| availability | 354（+451 内联） | entity | disabled_reason 状态机谓词 + 停用/恢复动作 + FailureCounter（**生产代码仅依赖 entity，健康**） |
| provider_model | 435 | entity | catalog/refresh/data |
| provider_template | ~1758（+699 tests.rs） | entity, crypto, db | 种子模板 + 默认头 + 接口类型 |
| provider_repo | 115 | entity | 供应商仓库 |
| stats_snapshot | 2144（含 math/generator_tests 等） | app_settings, db | 快照 core/registry/generator/reader/subject/tasks/math |
| cron | 3239（+1238 内联/独立测试） | i18n, entity, app_settings, config, db | 调度/worker/日志链路/seed |
| proxy | ~9081（+2046 测试） | provider_model, usage, response, entity, availability, state, auth | /v1 转发全域（19 生产文件 + convert/） |
| usage | 5174（+2678 内联） | availability, crypto, provider_template, proxy, entity, i18n, app_settings, state | 厂商用量抓取/缓存/门控（11 fetchers + 协议助手） |
| routes | 8030 | 16 个目标模块 | 全部 API 端点（14 生产文件 + stats/ 8 子文件） |
| state / lib / main | 35 / 320 / 10 | 组合根 | AppState 聚合 + 生命周期 + handler 注册 |

### 1.2 生产依赖图要点（已剥内联测试）

- 底层干净：entity/response/i18n/crypto/config/static_assets 无任何出边；availability 仅依赖 entity。
- 双层结构健康：proxy/usage/stats_snapshot/cron 是「能力层」，routes 是唯一「入口层」直连能力层（routes→16 目标，正常形态）；lib/state 是组合根。
- **usage↔proxy 双向边**：proxy→usage 8 处（usage_rank/lb/failure_* 消费 mem_cache/types），usage→proxy 3 处全部集中在 `usage/persist.rs:258/279/299`（probe_boundary_providers 调 `crate::proxy::probe_provider`）。唯一反向用途是「边界探活需发实测请求」。观察：探活能力（proxy/probe.rs 的 test_model/probe_provider）被 proxy 测速端点、usage 边界探活、failure_recovery 三方消费，属跨域能力，方向问题留给 05/07 票与 14 票交集处定夺。
- **auth→state（auth/mod.rs:26）**：auth 中间件/处理器经 `State<AppState>` 取 db。state 聚合了 proxy 连接池/调度器等，auth 对组合根整体耦合（axum 惯用法，但 auth 本可只依赖 DatabaseConnection——观察级，归 12 票）。
- **backup→auth（backup.rs:16）**：`use crate::auth::hash_token`——会话令牌哈希工具住在 auth 模块却被备份域消费；hash_token 属 crypto 域工具却无家可归（观察级，归 12/15 票交集）。
- proxy→cron 与 availability→cron/crypto/db、usage→cron 等边经复核全部为**内联测试代码**误报，生产图无这些边。
- 无反向/循环依赖（除上述 usage↔proxy 单点）。

### 1.3 文件归属观察（跨域代码的寄居）

- **`proxy/failure_recovery.rs`（270 行）**：cron handler（lib.rs:196 注册 FAILURE_RECOVERY_JOB）的实现体，语义属「供应商失败恢复生命周期」，寄居 proxy/ 下。→ 归属待 02/08/14 票确认（它调用 availability 谓词 + provider_model 查询 + crypto，与转发域无涉）。
- **`proxy/failure_recheck.rs`（115 行，RecheckGate）**：被 state.rs 注入；节流消费方在转发路径（待 02 票确认调用面）。
- **`usage/estimate.rs`（111 行）**：用量预估纯核心（C5 收敛产物），属 usage 域 ✓。
- **`usage/error.rs`（97 行）**：UsageError 定义于 usage、HTTP 映射散落在 `routes/providers.rs` 注释约定——错误→状态码映射只有文档链接没有代码单源（观察级，归 06/11 票）。
- **`state.rs`（35 行）**：从 availability（FailureCounter）/proxy（LbState/RecheckGate/UpstreamPool）/usage（UsageMemCache）/cron（SchedulerRuntime/log_tx）/app_settings 聚合——组合根形态健康。

### 1.4 文档漂移（AGENTS.md 结构树 vs 磁盘）

AGENTS.md 项目结构树落后于代码，以下文件/目录未收录（仅列事实，供后续 AGENTS.md 刷新批处理，图内不改）：

- proxy/：`failure_recovery.rs`、`failure_recheck.rs`、`dispatch.rs`/`relay.rs` 已收录 ✓；`tests.rs` 未收录
- routes/：`api_keys.rs`、`backup.rs`、`chat.rs`、`provider_templates.rs`、`request_logs.rs` 未收录；routes/stats/ 下 `rank.rs`、`rank_snap.rs`、`metrics.rs`、`compute.rs`、`window.rs`、`tests.rs` 与树描述不符（树写的是另一套拆分名）
- usage/：`error.rs`、`estimate.rs`、`sensenova_login.rs` 未收录
- stats_snapshot/：`math.rs`、`generator_tests.rs` 未收录
- cron/scheduler/tests.rs 未收录；provider_model/ 的 `catalog.rs`/`data`/`refresh.rs`、provider_template/seed 未收录
- 后端测试计数/分布描述与磁盘有出入（如 usage 内联测试 2678 行远超文字描述）

## 2. 后端散落候选（子代理扫描，全部锚点逐条复核）

| # | 家族 | 证据（已复核） | 判定 | 主票 |
| --- | --- | --- | --- | --- |
| F1 | disabled_reason 状态字面量 | 枚举单源 `availability.rs:30-47`（DisabledReason::as_str），但 `proxy/failure_recovery.rs:10/33/235`、`usage/persist.rs:246` 仍手写 `"failure"`/`"quota"` 字面量；`routes/providers.rs:375-377` 已走枚举 | 非单源：4 处绕开已有枚举 | 14（收口消费方），07 顺带 |
| F2 | cron_job_run.status 字面量 | `entity/cron_job_run.rs:15` 裸 String；写入 `cron/worker.rs:246`、`cron/log_repository.rs:158/234/263`，读取 `routes/cron_jobs.rs:341`；跨模块传裸串（log_repository.rs:73 trait 签名） | 零单源，同域三值域 | 08（建 RunStatus 枚举） |
| F3 | 用量缓存新鲜度 600s | `usage/persist.rs:21` TTL 常量 + `cache_age_fresh/_at`（:68/73-78）单源，mem_cache/usage_rank/routes 均复用 | **已单源**（免票，仅存此记录） | — |
| F4 | 时区偏移表达式 | 同一 `offset_from_utc_datetime→local_minus_utc` 表达式 3 份：`routes/stats/window.rs:35-42`（pub(crate)，注释自称与 stats 同口径）、`stats_snapshot/tasks.rs:32-36`（注释自称同口径源）、`cron/parser.rs:180-187`（FixedOffset 化）；时区值本身单源 `app_settings.rs:262 timezone_sync()` | 非单源：表达式 3 份 | 10（window.rs 是自然 owner，09/08 轴内顺带） |
| F5 | 协议编号分类法 | 三处独立列举：`entity/provider_template.rs:7-17 ProtocolType`（DeriveActiveEnum，**全仓零生产消费=死码**）、`provider_model/refresh.rs:8-11` PROTOCOL_* 常量（lb.rs:15-22 的 from_i32 消费它）、`entity/virtual_model.rs:29-34` INTERFACE_*；裸数字 `backup.rs:310/337/365-395`（0..=4 校验）、`provider_repo.rs:127` Set(0) | 非单源：分类法三写、枚举死码、备份裸范围 | 14（主），13/15 顺带 |
| F6 | 探活/测试请求构造 | 构造单源 `proxy/probe.rs:19 test_model`（routes/provider_models.rs:839、failure_recovery.rs:95、probe.rs:187 内部互调）；编排壳各写（probe.rs:163-205 vs failure_recovery.rs gate） | **构造已单源**；编排小壳差异归寄居文件问题（见 §1.3 与 08 票） | 04（顺带） |
| F7 | extra `usage:true` 谓词 | 单源 `usage/mod.rs:29 usage_enabled`（消费 failure_recheck/failure_recovery/persist/routes）；但 `routes/providers.rs:42` 另定 EXTRA_USAGE_KEY 并在 :259-263 重推同谓词 | 非单源：双实现 | 06（主），11 顺带 |
| F8 | 上游 URL 版本段拼接 | `proxy/convert/mod.rs:252-266 build_upstream_url` 支持 vN（v1/v1beta/v1alpha + 纯数字版本段，火山 v3 教训已入注释）vs `provider_model/refresh.rs:21-32 build_models_url` 只认 v1/v1beta/v1alpha 子集；convert 注释自称「沿用 build_models_url 规则」（实为反向：convert 更全） | 同族双实现 + 误导性注释 | 05（主），14 顺带 |
| F9 | request 表直查原始 SQL | format! 拼 where_sql 多套无共享 helper：`routes/request_logs.rs:203-236`、`routes/providers.rs:927-934`、`routes/stats/rank_impl.rs:60/155/272/458/615/688`、`summary_charts.rs:89/458/609/667`、`metrics.rs:23`；cron 表侧已有 helper（cron_jobs.rs:283/298/333 → log_repository） | 同域（routes 内 request 聚合）多套 | 11（主，request_logs 本体），10 顺带 |
| F10 | 中文文案不走 i18n | proxy/ 与 usage/fetchers 的 Lang 消费密度为 0（对照 routes/providers.rs 12 处、cron/worker.rs 9 处）；典型 zh-only：`proxy/native.rs:62/76`（/v1 下游错误体）、probe.rs:169-180 Skip 原因、failure_recovery.rs ProbeGate 文案 | i18n 模式本身单源（Lang::tr）；缺口在 proxy/usage 域 | 02/04/06 轴内顺带记录 |

### 2.1 三个文件归属结论（复核后）

- **A. `proxy/failure_recovery.rs`（270 行）名不副实成立**：唯一生产消费 = lib.rs:207 的 cron handler 注册（FAILURE_RECOVERY_JOB，seed @hourly）；转发路径零调用。本体 = cron 任务 + availability/probe 编排（调 test_model:95、availability::recover_probe:104）。寄居 proxy/ 属历史残留。→ 主票 08 做归属审查。
- **B. `proxy/failure_recheck.rs`（115 行）位置合理**：trigger 由转发失败链调用（lb.rs:47 note_member_failure 内，复核确认），body 语义 = 用量门控代理（调 usage::persist fetch_and_store/apply_usage_gate）——proxy 内的 usage 桥，属设计内。
- **C. `availability.rs` 是健康的被依赖底座**：生产 API = DisabledReason/as_str、traffic_available（消费 lb.rs:152/forward.rs:169）、FailureCounter（record_failure 经 on_forward_failure 由 lb.rs:40 链入，reset 由 failover.rs:345/dispatch.rs:137）、disable_for_quota/recover_quota（usage/persist）、enable_manual/disable_manual（routes/providers.rs:498/500）、recover_probe（failure_recovery）、set_items_enabled 仅内部使用；只依赖 entity。无归属混乱。
- **D. usage 子模块边界干净**：estimate.rs 仅 routes/providers.rs:917/953-955 消费；error.rs 是 usage 域公共错误（HTTP 映射仅文档化于 routes 注释——映射代码未单源，轻微，归 11 顺带）；http.rs 严格内聚零外引。

## 3. 前端域分析（web/src，子代理扫描 + 抽查复核）

### 3.1 结构事实

- **无 service/API 层**：`lib/api.ts`（100 行）只有 ky 实例 + ApiResponse 信封 + unwrap + fetchHealth（已抽查复核），**零端点函数**；全部端点 URL 内联在各 hook（如 use-request-logs.ts:83）；实体类型手写于各 hook（如 use-api-keys.ts:15 `ApiKeyDetail extends ApiKey`），无 codegen。
- **hooks 是隐式数据层**：28 个 hook 2272 行，职责 = 类型定义 + queryKey + 端点调用 + 状态管理捆绑；被页面直接依赖。
- 5 个赛马 hook 已收敛共享（lib/race-types + hooks/stats-query，抽查 ✓）。
- chat.tsx（387 行）自带内联流式 fetch（:136 `/api/chat/completions` + ReadableStream），不经过 ky 与共享 SSE（全站仅 use-cron-job-logs.ts:100 一处 EventSource）。

### 3.2 重复候选（各条均已核对锚点）

| 候选 | 证据 | 归属票 |
| --- | --- | --- |
| 弹窗脚手架样板（DialogHeader+ScrollArea+useForm+zod+toast 手写装配） | ProviderEditDialog.tsx:131（512 行）、VirtualModelEditDialog.tsx（685）、AddProviderModelsDialog.tsx（824）、ProviderModelDetailDialog.tsx（444）；仅 provider-model-form.tsx(57) 被复用 | 17（提炼框架给 19） |
| 概览页布局骨架同构（六指标卡+赛马卡+分析卡装配，每页 200-240 行） | provider-overview.tsx:81 / model-overview.tsx:44 / api-key-overview.tsx:46 / virtual-model-overview.tsx:106 + 共享卡组件已被复用 ✓ | 16（消费方，骨架收敛面） |
| 表格状态样板（SortingState/PaginationState/ColumnDef 各自管理） | RequestLogsTable.tsx:609、SettingsTable.tsx:270、ApiKeysTable.tsx:231；仅共享 data-table 3 原子组件 | 16/17/19 交界 |
| 图表系列色/中英文案内联各 config | dashboard-charts.tsx:28 CHART_COLORS/:36 OTHER_LABEL vs insight-charts.tsx:176-178/280-282/395 | 16 |
| i18n 绕过（硬编码中文未走 useTranslation） | settings 组件域 49 处（SettingEditDialog.tsx:69「更新成功」/:103「取消」等，抽查 ✓）、error-boundary.tsx:29-36、ProviderModelSection.tsx:71、ProviderUsageCard.tsx:64、dashboard-charts.tsx:84-91 | 20（重灾区=settings 组件域） |
| 端点契约手写同步（无类型生成） | web/src/types/ 仅 2 个 .d.ts；实体类型全在 hooks 手写 | 19 |

### 3.3 测试分布

55 测试文件：pages 17 个（约 125 用例，各概览页 5-7、api-keys 11/virtual-models 12/provider-models 10/overview 13）+ components 32 个（约 207 用例，provider-models-dialogs 33/provider-detail 22 最厚）。薄弱点：settings.tsx 页无测试、SettingEditDialog 无专属测试、providers 组件族大面积无直接测试（ProviderSpeedTestDialog/ProviderList/DeleteDialog）、hooks 几乎全裸（仅 use-in-view/stats-query/use-theme 有直测）、lib/api.ts 零测试、use-cron-job-logs EventSource 分支（:100）无测试。

### 3.4 前端 5 票切分修正建议

- **16 内部 cohesion 成立**（4 概览页 + race-window-control + insight-analysis-card 互引网络密）；但 16 与 17 经 hooks 深度互引（RequestLogsTable.tsx:30-35 import 全部 4 个 CRUD 实体 hook 做筛选下拉）——两票边界仍可保持，但盘点确认：**hooks 是两域的共享接口面，任一票开工前须视 hooks 为冻结接口**。
- **18 应拆为两组**：cron-jobs+use-cron-job-logs（SSE 域）与 login/auth/chat（会话域，chat 无 SSE、与 cron 零耦合证据）。
- **19 应缩小**：hooks 归属其消费域（16=stats/race/dashboard hooks；17=CRUD hooks），19 只留 ui/data-table/通用组件/lib/App——否则 16/17 无法独立交付。
- **20 与 18 的 settings 域重叠**：i18n 硬编码重灾区在 components/settings，建议 20 票范围显式含 settings 组件文案改造。

## 4. 结论：票切分修正（01 票拍板，2026-09-09）

**后端 02-15 切分维持，两处修正：**

1. **11 票范围补漏**：routes/ 目录实际 14 文件，原 11 票范围漏了 5 个——`api_keys.rs` / `request_logs.rs` / `provider_templates.rs` / `backup.rs` / `chat.rs` 全部补入 11（管理 API 域）。request 表直查家族（F9）主审在 11。
2. **08 票纳入寄居文件**：`proxy/failure_recovery.rs`（cron handler 实现体寄居 proxy/）由 08 票做归属审查，02 票不再审该文件；`proxy/failure_recheck.rs` 判定位置合理（lb.rs:47 消费），02 票仅按转发路径正常审查。

跨域家族主票映射：F1→14、F2→08、F4→10、F5→14、F7→06、F8→05、F9→11、F10→02/04/06 顺带；F3/F6 已单源免票（结论存本文件）。

**前端 5 票切分修正：**

1. **18 拆出 21**：login/auth/chat + RequireAuth 认证守卫 + use-auth/use-init-settings → 新 21「FE 会话与演示域」；18 缩为 cron-jobs + settings（含 use-cron-jobs/use-cron-job-logs/use-settings 与 SSE 域）。
2. **19 缩小**：hooks 归各消费域（16=stats/race/dashboard hooks；17=CRUD hooks；18=cron/settings hooks；21=auth hooks），19 只留 ui/data-table/通用组件/lib/types/App/main/setup——否则 16/17 无法独立交付。
3. **16/17 维持但 hooks=冻结接口**：RequestLogsTable.tsx:30-35 跨域 import 4 个 CRUD hook，任一票开工前 hooks 视为冻结接口（范围改动需跨票协商）。
4. **20 显式含 settings 组件文案域**（i18n 绕过重灾区 49 处 + chart label + error-boundary）。

**文档漂移（§1.4 清单）**：AGENTS.md 结构树落后于磁盘（routes/usage/provider_model/stats_snapshot 多处未收录、proxy 缺 failure_recovery/failure_recheck、测试分布描述失真）——随图后实施批次统一刷新，图内不改。

**测试薄弱面（供各票测试轴参考）**：后端 = cron_job_run 状态无单测枚举、F1 收口无谓词测试锚点、tz 表达式 3 份无共用单测；前端 = settings 页/SettingEditDialog/ProviderSpeedTestDialog 等无直测、hooks 全裸、lib/api.ts 零测试、EventSource 分支无测试。
