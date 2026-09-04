# 01: 后端 stats 加 apiKey 过滤 + 新增 api-key-metrics 端点

**What to build:** 让后端统计接口能按调用方 API Key 过滤，并提供「单个 key 的 6 指标」聚合端点，供新数据面板消费。
所有现有调用方不传新参数时行为完全不变（向后兼容）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `GET /api/stats/charts` 与 `GET /api/stats/insight`（共用查询参数结构）新增可选 `apiKey`，按 `r.api_key_name` 精确过滤；缺省不过滤
- [ ] `provider-rank` / `virtual-model-rank` / `provider-model-rank` 三个排行端点各新增可选 `apiKey` 过滤
- [ ] 新增 `GET /api/stats/api-key-metrics`：入参 `apiKey` + `startTime`/`endTime`，返回该 key 的 6 指标单行（请求数 / 总 token / TTFT / 平均耗时 / TPS / 缓存命中率，口径同 model_metrics）；参数缺失返回 400
- [ ] 集成测试：两个不同 `api_key_name` 的 request 行，断言各端点加 `apiKey` 后只聚合目标 key；api-key-metrics 有/无数据、跨 key 隔离、缺参 400
