# 0024 — 统计口径排除探测/测速流量

## Status

accepted

## Context

`test_model` 是全部探测/测速流量的唯一写库入口：连续失败恢复探测（`failure_recovery` 每小时）、用量边界实测探活（ADR-0010）、模型弹窗测速。三条入口写入 `request` 表时统一带 `virtual_model_id=0`、`api_key_name='test'` 标记（ADR-0010 决策「与正式流量同口径落库」仅指写入，未限定统计）。

永久不可恢复的供应商会让恢复探测每小时贡献一条**必失败**记录，持续累积后显著拉低概览页与数据面板的成功率——这些数字不再反映真实服务流量质量。ADR-0010/用户指南此前记录「探测/测速请求照常计入数据面板」，本决策推翻该口径。

## Decision

1. **统计读侧统一排除**：数据面板全部 12 个 stats 端点（summary/charts/insight/5×rank/4×metrics）的实时 SQL 与快照生成器（ADR-0021）的 WHERE 链统一追加基础谓词 `AND r.api_key_name != 'test'`。谓词文本由 `stats_snapshot::registry::base_predicate()` 单一事实源给出，直接引用 `proxy::TEST_API_KEY_NAME` 常量拼接——标记若变编译期联动，不产生第二份手抄。
2. **写入侧不变**：探测/测速请求照常落 `request` 表（请求日志页保留审计价值，`request_logs` 路由不加过滤）；`test_model` 三条入口的落库行为零改动。
3. **存量快照按口径版本重算**：`snapshot_meta` 新增 `predicate_version` 键（当前 `2`；隐含旧口径视为 1）。生成任务启动检查时版本缺失/落后即与时区变更同路径——清空快照全量回填，然后写入新版本。口径谓词再变时递增版本号。
4. 共享过滤助手里注入：`filter_parts`（charts/insight 共用）与 `push_rank_filters`（rank 端点共用）起始即带基础谓词，少数手工拼 WHERE 的站点（summary_live、metrics_prims、vm-member-rank、api-key-rank、generator 两处）单独追加同一 helper。

## Consequences

- 概览页成功率、数据面板全部图表/排行/指标只统计真实服务流量；恢复探测对永久不可用供应商的持续失败不再污染数字。
- 探测行在快照的 whole/provider/model/virtual_model 主体上的历史贡献由版本重算一次性清除（此前 `api_key` 主体本就因 JOIN 映射不到而不产行，不变）。
- 请求日志页仍可见全部探测记录（含失败原因），排障能力不受影响。
- 已否决的备选：给 `request` 表加显式 `is_test`/`origin` 列（迁移 + 回填 + 写侧三处构造改动）——效果与现有标记完全等价，成本高于收益。

实现：`src/stats_snapshot/registry.rs`（谓词单源）、`src/stats_snapshot/tasks.rs`（版本重算）；回归：`tests/stats_integration/exclude_probe.rs`、`stats_snapshot::tasks::tests::predicate_version_change_triggers_recalc_and_excludes_probe`。
