# 03: 快照生成器（闭桶固化）

**What to build:** 对任意已闭口桶执行固化：按主体模式分组的逐遍扫描（每模式一个 GROUP BY） request 行产出全部主体模式行（whole/provider/model/virtual_model/api_key/virtual_model_member/api_key_model；model 与 Key 类经 provider_model/api_key 按 id 映射 JOIN，映射不到的请求只进 whole/provider/virtual_model 行），Rust 侧按「桶×主体」计算 ttft/request_time 分位标量（仅 hour/day，月/年不存），恒写 whole 哨兵行（空桶全 0）；每桶整批一个事务，UNIQUE 幂等 upsert 可重跑。生成器集成测试（内存库造跨桶/空桶/失败/多主体/同名模型/api_key 已删/迟到/分位分布数据）。

**Blocked by:** 01（表与实体）、02（注册表与闭桶判定）

**Status:** completed (2026-09-09 实现并验证)

- [ ] 生成结果与注册表口径等价：同一闭桶跑生成 SQL vs 各端点实时 SQL 逐指标同值
- [ ] 幂等：同桶重跑/跨时区重跑不产生重复或脏行；事务中途失败不留半桶
- [ ] 分位标量精确等于对桶内原始值排序计算；空桶有 whole 全 0 哨兵行
