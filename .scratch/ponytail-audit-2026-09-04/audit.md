# Ponytail 全仓过度工程审计 — 整改跟踪

来源：2026-09-04 ponytail-audit 全仓扫描（后端 src/ + 依赖清单 + 前端 web/src/ 三路并行）。
执行 worktree：`../llm-gateway-ponytail-audit`，分支 `refactor/ponytail-audit`。

## 流程约定

- 每项修改后跑全量质量门：`cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets && (cd web && pnpm lint && pnpm vitest run)`
- 🟢 低风险：逐项修改 → 全量质量门 → 全部完成后集中提交一次
- 🟡 中风险：每项先调研具体方案再改 → 全量质量门 → 全部完成后集中提交一次
- 🔴 高风险：每项先调研再改 → 全量质量门 → 逐条单独提交
- 每完成一项在下表更新状态（✅ 完成 / ⏭️ 跳过及原因 / ⚠️ 部分完成及说明）

## 🟢 低风险（纯删除，无行为变化）

| # | 条目 | 位置 | 状态 |
|---|------|------|------|
| L1 | 删 `useCronStats` 整个文件（零调用方） | web/src/hooks/use-cron-stats.ts | ✅ |
| L2 | 删 sidebar.tsx 未使用导出（SidebarRail/Input/Separator/GroupAction/MenuAction/Badge/Skeleton/MenuSub*/useSidebar；保留 MenuButton 内部依赖），并按需删 import | web/src/components/ui/sidebar.tsx | ✅ |
| L3 | 删 `InsightData.failureReasons` 字段及类型（仅测试 mock 引用，从未渲染） | web/src/hooks/use-dashboard-insight.ts:26-44 | ✅ |
| L4 | 删未使用导出 `ResolvedTheme`（改文件内私有类型） | web/src/hooks/use-theme.ts:6 | ✅ |
| L5 | 删 `metrics::Stopwatch` + `StreamMetrics::non_stream_output_ms`（零生产调用） | src/proxy/metrics.rs:52-54,175-197 | ✅ |
| L6 | 删 `config.api_key_encryption_key` 字段（无任何读取，crypto 直读环境变量） | src/config/mod.rs:35,52-55 | ✅ |
| L7 | 删 `convert::actual_model_id`（定义后无调用） | src/proxy/convert/mod.rs:226-228 | ✅ |
| L8 | 删 `auth::session_cookie()`（无生产调用；内联至唯一测试调用处） | src/auth/mod.rs:162-164 | ✅ |
| L9 | 删 `provider_template::find_by_domain`（仅测试调用，测试内联等价实现） | src/provider_template/mod.rs:290-299 | ✅ |
| L10 | 删 `crypto::encryption_enabled()`（唯一调用处内联等价判定） | src/crypto/mod.rs:22-26 | ✅ |
| L11 | `tower` 移到 dev-dependencies（src 零引用，仅集成测试用） | Cargo.toml | ✅ |
| L12 | 删 `@dnd-kit/utilities`（内联等价 `transformToString`） | web/src/components/providers/ProviderList.tsx | ✅ |
| L13 | 删 4 个 Section 包装组件，overview 页直渲 Card | web/src/components/*-race/ + pages/overview.tsx | ✅ |
| L14 | 删 `SchedulerRuntime::new`/`JobWorker::new`/parser 非时区包装，测试改显式传参 | src/cron/{scheduler,worker,parser}.rs | ✅ |

## 🟡 中风险（合并重复/替换写法，行为应等价，测试兜底）

| # | 条目 | 位置 | 状态 |
|---|------|------|------|
| M1 | 6 处重复的 RaceSortKey/RaceWindow/RaceSort 类型收敛到 lib/race-types.ts（re-export 保持调用方不变） | web/src/lib/race-types.ts + 6 hooks | ✅ |
| M2 | provider_extra 回填抽共享 `backfill_host_extras` 管线（Krill/SenseNova/模板首插共用，-约100行） | src/provider_template/mod.rs | ✅ |
| M3 | ~~stats 4 排行榜 handler 收敛~~ | ⏭️ 跳过：member_rank 为左表反查本质不同构，前 3 者亦各有 SELECT/响应差异，收敛需引入 trait/泛型排序，净收益低于估计 |
| M4 | ~~`dispatch_success` 4 份手写 RequestRecord insert 收敛~~ | ⏭️ 跳过：4 构造点分属 4 协议/异步上下文，差异字段无法收敛，14 参数构造器反而损失字段名可读性 |
| M5 | `is_unique_violation` 五处同体复制收成共享 util | src/routes/{auth,api_keys,providers,provider_models,virtual_models}.rs | ✅ |
| M6 | fetcher 重复「401/403→Auth」判定统一到 `ensure_not_auth_error`（alibaba×2/cloud_balance×2/volcengine 复用） | src/usage/fetchers/{alibaba,cloud_balance,volcengine}.rs | ✅ |
| M7 | `catalog()`/`search()` 重复 RawModel→CatalogEntry 转换抽共享 `entry_from` | src/provider_model/catalog.rs | ✅ |
| M8 | ~~删 `useToastActions`，调用方直用 sonner~~ | ⏭️ 跳过：`toastError` 是有效两参封装（自动带 error.message），29 调用文件 + 测试 mock 改造成本远大于删 13 行收益，属真实价值抽象非 yagni |
| M9 | `insight-charts` 删本地重复 `formatBucketLabel`、改复用 `dashboard-charts` 导出（含 `inferGranularity`） | web/src/components/{insight,dashboard}-charts.tsx | ✅ |
| M10 | model-overview 手写指标卡改复用 `MetricsSummaryCard`（-约60行，与三级页统一） | web/src/pages/model-overview.tsx | ✅ |
| M11 | 小合并包：ProviderDetail/CronJobDetail 重复 formatDate 抽到 lib/utils.formatDateTime（⚠️ skeleton 泛化/schema 合并子项评估跳过：skeleton 网格列数不同、login/init 验证强度本不同） | web/src/{lib/utils.ts,components/providers/ProviderDetail.tsx,components/cron-jobs/CronJobDetail.tsx} | ✅（部分） |
| M12 | sea-orm 关 default-features 裁 rust_decimal（已验证依赖树移除） | Cargo.toml | ✅ |
| M13 | ~~分页 useEffect 复位改数据 key 驱动~~ | ⏭️ 跳过：react-table 受控分页无法派生替代，该 effect 是有意的「搜索后回第一页」UX 保护（官方 autoResetPageIndex 等价），移除有行为回归风险 |
| M14 | 删 `clsx`，`cn()` 内部改原生 classNames（等价实现） | web/src/lib/utils.ts | ✅ |

## 🔴 高风险（碰运行时语义/存量数据，逐项核实后单独提交）

| # | 条目 | 风险点 | 位置 | 状态 |
|---|------|--------|------|------|
| H1 | 删 `usage::UsageCache` 内存层 + `AppState.usage_cache` + routes 两处 invalidate | invalidate 时机影响用量新鲜度，读写两侧语义需核实 | src/usage/mod.rs, state.rs, routes/providers.rs | ⬜ |
| H2 | 删 `UsageData.kind`/`UsageKind` 枚举 | usage_json 落 DB 缓存，若 kind 被序列化则存量行反序列化可能失败 | src/usage/types.rs:11-32 | ⬜ |
| H3 | db.rs 迁移 13/16/17/19/20 样板收敛 | 碰启动迁移路径；生产有 schema_migrations 号段坑前科 | src/db.rs:328-425 | ⬜ |
| H4 | 删 `lib.rs` 注册的 `example` handler（无种子行） | AGENTS.md 记载其「演示实时日志」用途，需确认 | src/lib.rs:156-172 | ⬜ |

## 明确不做

- recharts 换手写 SVG：工程量大、动图表 UX，单独立项再议。

## 提交记录

（随进度回填；低风险批完成后集中提交）
