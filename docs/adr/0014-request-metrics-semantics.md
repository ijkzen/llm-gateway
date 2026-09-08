# 0014 — /v1 请求指标落库口径（request 表）

## Status

accepted

## Context

网关是请求的唯一观测点，数据面板、排行与用量预估全部基于 `request` 表的落库指标，口径必须自洽且稳定。早期存在两套易混的语义：独立 `network_latency` 字段（仅统计建连耗时）与 ttft 是否含建连的歧义；观测者容易把「TTFT 起点」理解错，导致耗时/tps 解读偏差。

## Decision

1. 每次转发成功/失败各落一行 `request`（含 usage 旁路扫描请求），字段语义以 `src/entity/request.rs` 头注释为唯一权威（口径改动必须同步该注释与消费端，不另造字段）。
2. **TTFT 起点 = 建连开始（新建连接）或请求发出（复用连接）**——建连耗时并入 ttft，无独立 `network_latency` 列（迁移 10 删除）。ttft 是流式「从起点到收到首个内容块」的耗时。
3. tps：流式 = `output_tokens / (ttft + 输出耗时)`；非流式 = `output_tokens / (end_time − 请求发出)`。`output_tokens` 含推理/思考 token（usage 缺失为 NULL）；缓存命中 token 记入缓存字段（read/creation 口径）。
4. 指标只增语义解释，不向后改写；消费端聚合一律基于已落库字段（新增聚合零 schema 变更，见 ADR-0015）。

## Consequences

- 时钟与起点口径单一：新建连接/复用连接统一解释，负缺口等异常先怀疑时钟混用而非逻辑。
- 表字段就是契约——新字段先迁移+注释，聚合端只读。
- 网关记录是「已用 token」的唯一可信来源（用量预估信任边界的基础，见 ADR-0008）。
