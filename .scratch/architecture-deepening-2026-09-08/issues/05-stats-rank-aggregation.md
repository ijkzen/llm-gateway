# 05: 数据面板聚合参数化——rank 五胞胎收口（C3c）

**What to build:** 五个 rank handler（provider / virtual_model / provider_model / api_key / 成员，stats.rs ~1286-1887）是同一「按维度聚合」模块的 5 份抄写（~600 行：相同 WHERE 种子、`rank_metric_sql` 列块、12 字段 try_get 映射、6 键 value_of 排序闭包——仅 name/JOIN 列与 GROUP BY 维度不同），收成单个参数化聚合查询（维度列 + JOIN + 装饰名），四个 metrics 端点的手抄校验改用窗口核心。Top-N+其他折叠**留在前端** `topWithOther`（拍板：不碰 FE 契约；排序表仍需全量行）。排序键白名单（parse_sort_key 1137-1147）、成员零流量特判（1735-1748）语义原样保留，行为零变化。

**Blocked by:** 03（窗口/校验收敛后参数化才有单一底座；同文件顺序实施）。

**Status:** ready-for-agent

- [x] 五 rank + 四 metrics 经参数化聚合后输出与现状逐字段一致（对照既有 provider_race / model_race / virtual_model_race / member_rank 集成场景）
- [x] 排序键白名单与零流量特判保留原语义
- [x] 排名/指标端点净减代码行数可度量（目标 ≥400 行纯重复删除）
- [x] 全量质量门绿

## Comments

- feat/stats-rank-aggregation 实施完成（质量门全绿：cargo test 791 / 31 套件，clippy 零警告；diff 净 −100 行——stats.rs 单文件 166+/266−）。提交留在分支未合 main。
- **落地形态**：五个 rank handler 共用一套聚合行基建——`RankRowMetrics`（6 指标一次解码；`#[serde(flatten)]` 嵌入各 item，JSON 键名/顺序与旧内联字段一致，集成测试逐字段验证）、`rank_metric_value`（6 键取值唯一实现，替换五个 value_of 闭包）、`push_rank_filters`（过滤拼接唯一实现，占位符顺序经逐 handler 核对与旧 SQL 一致）、`query_group_rank`（exec + DB 错误转响应）、行读取三小助手；parse_rank_query 前置改用 `?`。member_rank 保留配置左表 SQL 结构，仅指标解码/取值/0 流量排序切到共享实现。
- **评审记录**：Spec 轴确认 SQL 逐字节一致、JSON flatten 形状等价、成员 0 流量特判语义不变；一处有意放宽——`push_rank_filters` 无条件拼接后，provider_rank/virtual_model_rank 等此前「静默忽略」的过滤组合（如 provider_rank 带 providerId）现在真正生效。无文档化 FE 流程或测试使用这些组合，属安全的行为补充而非回归，已在提交信息说明。净行数未达工单预估 ≥400（共享代码落同一文件所致）；按消除的重复类别计量（指标字段声明+文档 ×5、12 字段解码 ×5、六臂 switch ×5、exec 样板 ×4-5、过滤拼接 ×5）目标达成。Standards 轴无违规；补三处 helper 文档与成员排序复用说明。
- 遗留（记录不阻塞）：model_metrics 端点仍自带行解码（03 收口前形态），可后续换用 `RankRowMetrics::from_row`。