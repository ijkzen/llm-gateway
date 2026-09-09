# 06: rank×5 + metrics×4 走快照

**What to build:** 五张赛马（provider/virtual_model/provider_model/virtual_model_member/api_key）与四个单主体指标端点改走公共读取器：六指标（request_count/total_tokens/avg ttft/avg request_time/tps/cache_hit_rate）由快照原语在闭桶段加总后现算（helper 与现 rank_metric_sql 口径同构），排序/方向/窗口返回形状不变；主体行按各自 entity_type 读取，model 级与 api_key 过滤先按 id 解析。等价测试（同上三态基准比对）。

**Blocked by:** 05（公共层）

**Status:** completed (2026-09-09 实现并验证)

- [ ] 5 rank + 4 metrics 三态下响应与实时基准恒等（含默认排序方向、asc/desc、任意窗口）
- [ ] api_key 过滤按 id 解析后取数，等价于现按名称过滤实时口径
- [ ] 现有赛马/指标集成测试全绿
