# 05: 读路径公共层 + summary/charts 走快照

**What to build:** 读路径公共读取器：按闭桶分解把窗口切成「快照闭桶集 + 兑底段」，闭桶从 EAV 行按 (entity_type, entity, metric_type) 取原语并按桶拼接、兑底段实时聚合 request 表后合并，比率/均值读时现算，补零与现接口一致；主体展示名 JOIN 现表解析（provider_name/api_key_name）。`/api/stats/summary`（任意窗口/全历史）与 `/api/stats/charts`（hour/day/month/year 趋势 + 按主体分布）改走公共层；缺快照（无哨兵行）的闭桶实时兑底。等价测试基建落地：同一批 request 数据先取全实时基准，再注入「部分闭桶快照 + 故意缺口/错位桶」断言响应与基准完全一致，补齐快照后仍一致。

**Blocked by:** 02（分解/注册表）、03（生成器可产真行）

**Status:** completed (2026-09-09 实现并验证)

- [ ] summary/charts 在「全实时（无快照）」「部分快照」「全快照」三态下响应与实时基准恒等（含月/年粒度与 Top10+其他分布）
- [ ] 现有 stats 集成测试全绿（表空→兑底，行为不变）
- [ ] 兑底与快照拼接口径无双重计数/漏桶（跨时区偏移桶、窗口边缘部分桶）
