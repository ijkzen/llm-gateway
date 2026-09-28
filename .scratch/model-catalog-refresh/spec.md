# Spec: 外置模型目录 + 每日刷新（model-catalog-refresh）

Status: ready-for-agent

## Problem Statement

模型目录（models.dev 全量元数据）目前以 vendor 文件编译期内嵌进二进制，匹配逻辑读的是这份只读快照。上游 models.dev 持续新增/调整模型，导致目录数据滞后：每次更新都要改仓库数据文件、跑全量质量门、重新发版与部署（最近一次从 379 条刷到 432 条就是完整走了一遍）。

管理员希望目录能**在运行中自动保持较新**，同时不牺牲内网部署的确定性：没有外置数据时仍能用内嵌快照工作，网络不可用也不影响匹配。

## Solution

引入「外置模型目录」覆盖层：匹配时优先读位于 db 目录父目录下的 `models.json`（由 `DATABASE_URL` 推导，prod 即 `/config/models.json`）；读不到就回退编译期内嵌快照。新增内置定时任务 `models_refresh`，每天 04:00（本地时区）从 `https://models.dev/models.json` 拉取全量目录，校验为合法非空 JSON 后原子替换外置文件；失败保留旧文件并把任务记为失败。不预热、不做状态接口、不做备份。

## User Stories

1. 作为管理员，我希望匹配模型时优先使用外置目录文件，这样不用改代码发版就能让智能填充用到更新的目录数据。
2. 作为管理员，我希望外置目录文件不存在时自动回退到内嵌快照，这样全新部署无需任何额外步骤即可正常工作。
3. 作为管理员，我希望外置目录文件损坏或不可读时也回退到内嵌快照，这样坏文件不会让智能填充整体失效。
4. 作为管理员，我希望外置目录文件位置与 db 目录平行（prod 为 `/config/models.json`），这样它和数据库一样落在持久化卷里、与数据库一同管理。
5. 作为管理员，我希望目录文件的路径由 `DATABASE_URL` 自动推导，这样自定义数据库路径时目录文件位置也随之正确。
6. 作为管理员，我希望有一个每天 04:00 自动执行的目录刷新任务，这样目录数据无需人工干预即可保持较新。
7. 作为管理员，我希望刷新任务从 models.dev 拉取与本仓库同格式的目录数据，这样写入外置文件的数据与内嵌快照结构完全一致、可直接被匹配使用。
8. 作为管理员，我希望刷新只有在下载内容是合法且非空的 JSON 时才写盘，这样断网半截响应或错误页不会污染外置目录。
9. 作为管理员，我希望刷新写盘是原子替换（临时文件 + rename），这样匹配不会读到写到一半的文件。
10. 作为管理员，我希望刷新失败时保留现有外置文件、匹配继续用已有数据，这样一次网络抖动不会让目录倒退。
11. 作为管理员，我希望刷新失败在定时任务页显示为失败并带错误原因，这样我能发现并手动重试。
12. 作为管理员，我希望能在定时任务页对 `models_refresh` 点「立即执行」手动触发刷新，这样部署后不必等到次日 04:00。
13. 作为管理员，我希望匹配在目录文件变更后无需重启即时生效（含手工替换文件），这样紧急更新目录无需重启服务。
14. 作为管理员，我希望外置目录文件与内嵌快照的结构、serde 映射完全兼容，这样切换来源不会引入解析差异。
15. 作为管理员，我希望 prod 部署把整个 `data/` 目录挂载到 `/config`，这样外置目录文件与数据库、日志一同持久化，容器重建不丢失。
16. 作为管理员，我希望目录刷新任务与其它内置任务一样出现在定时任务页（含中英文标题/描述），这样它可被查看、编辑、手动执行。
17. 作为开发者，我希望目录读取逻辑可注入外置路径，这样测试无需改动全局状态即可验证外置优先与回退。
18. 作为开发者，我希望刷新逻辑的源 URL 与目标路径都是参数，这样测试用本地 mock HTTP 与临时文件即可覆盖成功/失败分支，不触真实网络。

## Implementation Decisions

- **模块**：`provider_model::catalog`（读取与匹配，改造）承担「外置优先 + mtime/size 缓存 + 内嵌兜底」；新增 `provider_model::catalog_refresh`（刷新：抓取、校验、原子写、路径推导）；`cron::seed` 增加内置任务种子；`lib.rs` 增加 handler 注册与启动期路径注入；`db` 暴露 `DATABASE_URL → 文件路径` 的公共解析（复用现有 `ensure_sqlite_dir` 的口径，单一事实源）。
- **读取缓存**：`catalog` 的全局索引由「一次性 `OnceLock`」改为可重载缓存，持有「观测到的来源键 + 索引 + 原始列表」。来源键 = 外置文件当前的 `(mtime, len)`，或内嵌。每次读取先 `stat` 外置文件计算来源键，与缓存键一致则直接复用；不一致才重新解析并替换。外置读取/解析失败时，把「该来源键」连同**内嵌内容**一起缓存（避免同一坏文件每次请求重复读取与告警），文件一旦变化（键变）再重试。
- **读取 API 形态**：匹配相关公开函数保持名字与语义，但返回**拥有值**（条目结构可克隆）而非 `&'static`，因为缓存可被替换；公开函数委托给一个可实例化的 `Catalog`（含可选外置路径），并保留进程级全局实例供生产调用。测试直接构造带临时路径的实例，避免全局状态互相干扰。
- **路径推导**：从 `DATABASE_URL` 解析出数据库文件路径（`sqlite::memory:` 无文件 → 无外置），取该文件所在目录（db 目录）的父目录，拼接 `models.json`。dev（`sqlite://db/app.db`）→ 工作目录下的 `models.json`；prod（`sqlite:///config/db/app.db`）→ `/config/models.json`。启动时由 `lib.rs` 计算并注入 `catalog` 全局实例。
- **刷新函数**：`catalog_refresh::refresh_from(url, path)`——GET `url`，非 2xx 即错；把响应体交给 `catalog` 的校验入口（用与读取同一套 serde 结构解析，确保结构兼容）并断言条目数 > 0；通过后写同目录临时文件再 `rename` 覆盖 `path`（必要时先建父目录）；返回条目数。任何失败都不触碰 `path`。
- **源 URL**：常量 `https://models.dev/models.json`，以函数参数注入测试用 URL；不新增环境变量。
- **handler**：注册 `models_refresh`；进程级 `try_lock` 防重叠；无外置路径（如内存库）时告警并正常返回；失败返回错误交回 worker（记 failed + 追加系统日志）。成功后依赖 mtime 缓存自动生效，不做显式重载。
- **种子**：`models_refresh` 表达式 `0 0 4 * * *`、`group = system`、`enabled = true`、幂等 upsert；中英文默认标题与描述（如「模型目录刷新 / Model Catalog Refresh」）。
- **不预热**：启动不主动联网；外置文件由定时任务或手动「立即执行」生成。
- **部署**：`deploy/docker-compose.yaml` 的 volumes 由两条子路径挂载改为单条 `./data:/config`，host 布局保持 `data/db`、`data/logs`、新增 `data/models.json`；该 compose 文件需在部署机手动同步一次。
- **无 schema 变更**：不新增数据库表/列，不需要迁移。

## Testing Decisions

- 好测试只验证外部行为：给外置文件/HTTP 响应输入，断言匹配结果与文件最终内容，不断言内部缓存字段或私有函数。
- **catalog 读取 API**（`provider_model::catalog` 单元测试，构造带临时路径的实例）：外置文件存在时 `find_by_suffix` 返回外置条目而非内嵌；文件内容变更（mtime 变）后再次读取返回新内容；文件删除后回退内嵌（`entry_count` 回到内嵌条数）；非法 JSON 回退内嵌且不报错；路径为 `None` 时等同内嵌。先例：现有 `catalog` 的 `find_by_suffix` / `similar_entries` / `search` 单元测试。
- **刷新函数**（`provider_model::catalog_refresh` 测试，本地 mock HTTP + 临时文件）：合法 JSON → 文件被写入且返回条目数；非法 JSON / 非 2xx / 空对象 `{}` → 返回错误且**已存在的旧文件内容不变**；`sqlite::memory:` / dev / prod 三种 `DATABASE_URL` 的路径推导断言。先例：`usage` 系列用本地 mock 上游的集成测试、`proxy_integration` 的本地 mock 上游。
- **定时任务接线**（集成测试，参照 `provider_quota_gate_integration::usage_refresh_job_seed_is_scheduled` 与 `stats_snapshot_jobs_integration`）：注册同名 dummy handler + `ensure_models_refresh_job` 幂等执行两次 → 只有一行、表达式 `0 0 4 * * *`、`group = system`、`enabled = true`、`load_from_db` 后 `has_job` 为真。
- 三个测试缝均已与用户确认。

## Out of Scope

- 刷新源 URL 的环境变量开关（ponytail 裁剪，用户确认）：URL 为常量，测试用参数注入。
- 外置目录的状态查询接口与前端展示（用户确认）：不新增 `/api/model-catalog`，不展示上次刷新时间/来源/条数。
- 外置文件备份/历史版本（用户确认）：单文件、临时文件 + rename。
- 启动预热（用户确认）：不主动联网。
- 文件监听/热重载信号：mtime/size 检查已覆盖外部手改与定时刷新。
- 数据库 schema 变更、前端改动（定时任务页已有能力复用）。
- 目录数据的手动合并/裁剪、按供应商过滤等高级能力。

## Further Notes

- 本决策取代 ADR-0001 的「不做运行时更新」条款（内嵌文件仍作兜底），记录于 ADR-0025；`CONTEXT.md` 新增「外置模型目录」「目录刷新」词条。
- prod 部署的 compose 挂载变更不在 `deploy.sh` 自动同步范围内，需在部署机手动更新一次 compose 文件后重建容器。
- 内嵌目录仍会随版本更新（兜底新鲜度），外置目录是运行时覆盖层。
