# 0025 — 外置模型目录文件 + 每日刷新任务

## Status

accepted

## Context

ADR-0001 决定模型目录以 minified JSON vendor 进仓库、`include_str!` 编译期内嵌，并明确「**不做运行时更新**；目录过期时手动重新下载替换该文件并随版本发布」。代价是目录数据滞后于 models.dev 上游：每次更新都要改代码、跑质量门、重新发版/部署（见本次 379 → 432 条的手动刷新）。

上游 models.dev 提供稳定的 `https://models.dev/models.json`（扁平 `vendor/model` 键，与本仓库 vendor 文件同格式、同 serde 结构），使「运行时刷新」成为可行选项。需要决定：把刷新结果放哪、匹配时如何取用、如何避免每次匹配解析 400KB。

## Decision

1. **外置目录文件 = db 目录的父目录下的 `models.json`**，路径由 `DATABASE_URL` 推导（复用 `db.rs` 的 URL→文件路径口径）：dev 为工作目录下的 `models.json`，prod 为 `/config/models.json`。它是运行时覆盖层，不是新的配置源。
2. **读取顺序：外置优先、内嵌兜底**。每次匹配 `stat` 外置文件的 mtime/size，变更才重新解析并原子替换内存索引（`RwLock<Arc<…>>`），未变更复用缓存；文件缺失、读不到或解析失败一律回退内嵌快照。因此不引入文件监听/重载信号，外部手工替换与定时刷新都能即时生效。
3. **每日刷新任务 `models_refresh`**（`0 0 4 * * *`，服务器本地时区）：从 models.dev 拉取，校验为合法非空 JSON 对象后「临时文件 + rename」原子替换；任何失败保留旧文件并让任务记 failed。源 URL 以函数参数注入（测试传本地 mock URL），不加环境变量。
4. **不预热**：外置文件缺失时不主动联网，匹配先用内嵌数据，等定时或管理员手动「立即执行」生成。
5. **prod 挂载改为整目录 `./data:/config`**：`/config` 此前未挂载，写在 `/config/models.json` 会随容器重建丢失；整目录挂载让 `data/models.json` 与 `data/db`、`data/logs` 一同持久化。

## Considered Options

- **继续纯内嵌、手动发版**（ADR-0001 现状）：零运行时复杂度，但每次上游更新都要发版，本次已因此产生一次「改数据 + 改断言 + 发版」的全流程。
- **运行时直接抓取、无外置文件**：省掉持久化，但内网部署/网络抖动时匹配直接受影响，违背 ADR-0001 选内嵌的初衷。
- **每请求重新读文件**：字面满足「每次匹配先读」，但每个 /v1 请求解析 400KB JSON，延迟与 CPU 不可接受。
- **文件监听/显式重载钩子**：比 mtime 检查更复杂，且跨容器/手工编辑不可靠。

## Consequences

- 目录更新不再需要改代码发版：每日自动刷新；外置文件可人工替换立即生效（无重启）。
- 匹配行为在无外置文件时与 ADR-0001 完全一致（内嵌兜底），可用性不受网络影响。
- 外置文件是运行时状态：容器重建依赖 `./data` 挂载持久化；`deploy/docker-compose.yaml` 需在部署机同步一次（`deploy.sh` 只传二进制）。
- 二进制仍内嵌完整快照（体积不降），换取零依赖的确定性兜底。
- 已否决：状态查询接口/前端展示、外置文件备份/历史版本、刷新源环境变量——均由 ponytail 裁剪并经用户确认。

实现：`src/provider_model/catalog.rs`（外置优先读取 + mtime 缓存）、`src/provider_model/refresh.rs` 或同域模块（抓取+校验+原子写）、`src/cron/seed.rs`（`models_refresh` 种子）、`src/lib.rs`（handler 注册）；文档：ADR-0001 运行时更新条款由本 ADR 取代，`CONTEXT.md` 增「外置模型目录」「目录刷新」。
