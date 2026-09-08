# 05: 数据面板聚合参数化——rank 五胞胎收口（C3c）

**What to build:** 五个 rank handler（provider / virtual_model / provider_model / api_key / 成员，stats.rs ~1286-1887）是同一「按维度聚合」模块的 5 份抄写（~600 行：相同 WHERE 种子、`rank_metric_sql` 列块、12 字段 try_get 映射、6 键 value_of 排序闭包——仅 name/JOIN 列与 GROUP BY 维度不同），收成单个参数化聚合查询（维度列 + JOIN + 装饰名），四个 metrics 端点的手抄校验改用窗口核心。Top-N+其他折叠**留在前端** `topWithOther`（拍板：不碰 FE 契约；排序表仍需全量行）。排序键白名单（parse_sort_key 1137-1147）、成员零流量特判（1735-1748）语义原样保留，行为零变化。

**Blocked by:** 03（窗口/校验收敛后参数化才有单一底座；同文件顺序实施）。

**Status:** ready-for-agent

- [ ] 五 rank + 四 metrics 经参数化聚合后输出与现状逐字段一致（对照既有 provider_race / model_race / virtual_model_race / member_rank 集成场景）
- [ ] 排序键白名单与零流量特判保留原语义
- [ ] 排名/指标端点净减代码行数可度量（目标 ≥400 行纯重复删除）
- [ ] 全量质量门绿

## Comments
