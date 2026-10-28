# 数据面板与统计

数据面板的端点、统计口径与快照加速层。代码在 `src/routes/stats/` 与 `src/stats_snapshot/`。

## 端点

- `GET /api/stats/summary`：全量历史累计（请求数/成功率/总 token/加权缓存命中率）。
- `GET /api/stats/charts`：过去 24 小时按小时分桶趋势 + 按上游 `model_id` 分布。
- 另有 5×rank（provider/model/virtual_model/api_key 排行等）与 4×metrics、insight 端点，共 12 个。

前端 overview 页（侧边栏「数据面板」）用 Recharts + shadcn chart 展示 4 指标卡与两组三态图表（折线/饼图/降序条形图，Top 10 + 其他）。

## 统计口径：排除探测/测速流量（ADR-0024）

`test_model` 落库的探测行（恢复探测、边界实测、弹窗测速、模型测速页）统一标记 `virtual_model_id=0`、`api_key_name='test'`，**照常写入 request 表**（请求日志页可见），但全部 12 个 stats 端点与快照生成统一追加基础谓词排除。

排除谓词单一事实源是 `stats_snapshot::registry::base_predicate`（绑定 `TEST_API_KEY_NAME` 常量）。存量快照经 `snapshot_meta.predicate_version` 版本检查自动清空重算。

## 统计快照（`request_log_snapshot`，ADR-0021）

数据面板全部 stats 端点不再整窗实时扫 `request`：按设置表时区把已闭桶的时间片（hour/day/month/year 四级独立物化）固化，读路径只做「闭桶取快照 + 未覆盖增量兑底」。

- **表形态**：EAV 窄表（每行 = 桶 × 主体 × 指标名 × 值）。指标存加和/计数原语，闭桶分位存 p50/90/95/99 标量。
- **主体键**：一律用主键 id（model/api_key 生成时由 `(provider_id, provider_model_id)` / name 映射，映射不到的请求不产该主体行）。
- **生成任务**：`stats_snapshot`（@every 1h，首启/时区变更自动全量回填）+ `stats_snapshot_rebuild`（@every 1h，自愈补缺哨兵桶）。
- **兑底是第一类规则**：快照缺失的闭桶一律实时兑底——快照只是加速层，数字口径不变，绝不因缺快照返回错误或空数据。

改动统计语义时的对齐要求：

- 新增可加和指标 → 改 `stats_snapshot::registry`（单一事实源，读写两侧谓词同文本）。
- 改变排除谓词 → 递增 `PREDICATE_VERSION`，存量快照自动重算。
- 改完跑 `tests/stats_snapshot_integration.rs`：它断言快照路径与实时路径的响应**逐字节一致**；该测试红了说明快照漏算或口径分叉。
