# 需求：外置模型目录 + 每日刷新（model-catalog-refresh）

## 来源请求（用户原话）

> 添加一个定时任务每天执行一次定时任务，刷新 `models.json` 文件，并把它保存在一个外置存储位置，和 `db` 目录平行。
> 每次匹配时，先读取外置存储位置；如果读不到，就读嵌入的文件。

## 范围（Scope）

1. **外置目录文件**：模型目录的运行时数据落在「db 目录的父目录」下的 `models.json`：
   - 路径由 `DATABASE_URL` 推导——取数据库文件所在目录的父目录，拼接 `models.json`。
   - dev（`sqlite://db/app.db`）→ 进程工作目录下的 `models.json`；prod（`sqlite:///config/db/app.db`）→ `/config/models.json`。
2. **匹配读取顺序**：每次匹配先看外置文件——`stat` 其 mtime/size，变更才重新解析并替换内存索引，未变更用缓存；外置文件缺失 / 读不到 / 解析失败一律回退**嵌入文件**（`include_str!("data/models.json")`，当前 432 条）。
3. **每日刷新定时任务**：新增内置任务 `models_refresh`，表达式 `0 0 4 * * *`（服务器本地时区，生产即东八区，每天 04:00）。
   - 抓取固定源 `https://models.dev/models.json`（源 URL 以函数参数注入，便于测试传本地 mock URL）。
   - 校验下载内容为合法、非空的 JSON 对象后才写盘；写盘走「临时文件 + rename」原子替换。
   - 失败（网络错误 / 非 2xx / JSON 非法 / 空对象）→ **保留现有外置文件不动**，任务返回失败（worker 记 failed + 追加「任务执行失败：…」系统日志）。
4. **prod 持久化**：`deploy/docker-compose.yaml` 的 volumes 由两条子路径挂载（`./data/db:/config/db`、`./data/logs:/config/logs`）改为单条整目录挂载 `./data:/config`，使 `./data/models.json` 随容器重建持久化（host 布局仍为 `data/db`、`data/logs`、`data/models.json`）。该 compose 文件需在部署机手动同步一次（`deploy.sh` 只传二进制）。
5. **不预热**：外置文件不存在时不主动联网；匹配先用嵌入数据，等 04:00 定时或管理员在「定时任务」页点「立即执行」生成外置文件。
6. **文档对齐**：ADR-0001 的「不做运行时更新」条款由新 ADR 取代；`CONTEXT.md` 补外置目录词汇。

## 已拍板的决策（grilling 结论）

- **位置**：`DATABASE_URL` 推导的 db 目录父目录（非新环境变量）。
- **读取语义**：mtime/size 变更才重解析的缓存，不每请求全量解析 400KB。
- **调度**：固定低峰 `0 0 4 * * *`，不用 `@daily`/`@every 24h`。
- **刷新源与失败**：固定 models.dev/models.json；失败保留旧文件。
- **prod 挂载**：整目录 `./data:/config`（避免单文件绑定在 host 文件不存在时被 Docker 建成目录）。
- **损坏回退**：外置文件解析失败 → 本次匹配用嵌入数据（不删文件、不用上次缓存）。

## 非目标（Non-goals，ponytail 裁剪，均经用户明确确认）

- **不做刷新源 URL 环境变量**（用户确认「砍掉」）：URL 作常量，测试用函数参数注入本地 mock URL。
- **不做目录状态查询接口 / 前端展示**（用户确认「砍掉」）：无 `/api/model-catalog`，不展示上次刷新时间/来源/条数；状态看定时任务页执行日志。
- **不做外置文件备份/历史版本**（用户确认「砍掉」）：单一 `models.json`，临时文件 + rename 原子替换。
- 不新增数据库表、不做 schema 迁移。
- 不改前端（任务由已有定时任务页自动列出、可手动执行）。
- 不做运行时热重载信号 / 文件监听（mtime 检查已覆盖外部手改与定时刷新）。

## 事实约束（实现前已确认）

- `src/provider_model/catalog.rs`：现以 `include_str!` + `OnceLock` 一次性构建「尾段 → 条目」索引；`find_by_suffix` 返回 `Option<&'static CatalogEntry>`、`similar_entries`/`search` 返回拥有的 `Vec`。改为可重载缓存需让读取侧拿拥有值或 `Arc`（`CatalogEntry: Clone`）。
- `src/db.rs::ensure_sqlite_dir` 已有 `DATABASE_URL` → 文件路径的解析先例（`sqlite::memory:` 无文件 / `sqlite://rel` 相对 / `sqlite:///abs` 绝对）；外置路径推导应复用同一口径。
- 内置任务模式（`src/cron/seed.rs` + `src/lib.rs::init`）：常量任务名 → `default_title`/`default_description` 双语 → `ensure_*_job(db)` 幂等种子 → `scheduler.register_handler(name, handler)`；无创建任务 API。
- 出站 HTTP 先例：`reqwest`（`src/usage/http.rs`，15s 超时）。`tests/` 集成测试用本地 mock 上游，无需真实网络。
- 嵌入目录当前 432 条（见 `catalog.rs` 测试断言）；解析用 serde 结构要求每条含 `reasoning`/`tool_call` bool。
- prod compose 仅挂载 `/config/db`、`/config/logs`；`/config` 未挂载 → 写在 `/config/models.json` 容器重建即丢（故需改挂载）。

## 文档更新

- 新增 `docs/adr/0025-external-model-catalog-with-daily-refresh.md`。
- 更新 `docs/adr/0001-embed-model-catalog-as-vendored-asset.md`：标注运行时更新条款由 ADR-0025 取代。
- 更新 `CONTEXT.md`：模型目录词条改为「嵌入 + 外置」两层，新增「外置模型目录」「目录刷新」词条。
