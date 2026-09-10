# 0009 — 供应商用量查询：host 分发、归一化输出与数据库缓存

## Status

accepted

## Context

LB 选路、额度门控与详情展示都需要供应商用量数据。真实抓取各家用量接口慢、且各家凭据形态差异极大（API Key 直查 / Copilot OAuth / 火山与阿里 AK/SK 签名 / CookieCloud cookie 系），登录态接口还有风控；若每次需要都现抓，选路与展示会被外部接口拖垮，也会被打爆限流。展示与决策需要同一份稳定数据。

## Decision

1. 按 base_url 的 host（必要时再看 path，如火山方舟与阶跃的按量/订阅共用 host）分发 fetcher（`usage::fetcher_for`），各家差异收敛在 fetcher 层；输出归一化为 `UsageData`：订阅窗口（`QuotaWindow`，厂商不提供的窗口标 `available=false`）或按量余额条目（`BalanceItem`，fetcher 标记 `primary` 主余额）。
2. 数据库缓存（`provider_usage_cache`，迁移 9）：真实抓取经 `fetch_and_store` 落库；10 分钟内 `read_usage_cache` 直出，过期/缺失才真实抓取；`?refresh=1` 强制重取；更新/删除供应商时 `invalidate_usage_cache` 失效对应行。
3. 内置定时任务 `usage_refresh`（`@every 5m` 种子行）枚举全部「已开启用量展示」的供应商（**不过滤 enable**，停用的也持续监测，供额度恢复判定与人工查看）并发刷新落库，并顺带执行额度门控（见 ADR-0010）。
4. 展示与排序侧只读缓存不触发抓取（避免浏览即打外部接口）；无时区厂商的重置时间字符串按设置表 `timezone` 解释（`timezone_sync` 进程内同步副本随设置热更新，见 ADR-0015）。
5. **抓取单飞 + 失效代次护栏**（2026-09-10 修复 11-02/11-25）：管理端用量接口与 LB 兜底共用内存缓存的按 provider 单飞入口（`UsageMemCache::fetch_shared_stored` / `fetch_shared`），同一瞬间并发请求只打一次上游；`invalidate` 自增该 provider 的代次，抓取在开始前记录代次、写库与回填前比对——期间发生过失效（凭据已变）则结果作废（`UsageError::Stale`，502 类可重试），旧凭据数据不会写回刚失效的缓存。

## Consequences

- 决策与展示路径不因外部抓取失败/变慢而抖动：缓存兜底 + 定时刷新 + 复查节流（ADR-0012）三道防线。
- 新厂商接入 = 新增一个 fetcher，分发表与归一化类型不动；凭据一律进 provider extra（加密见 ADR-0002）。
- 各厂商规格细节（CookieCloud 父域、余额单位等）只活在 fetcher 与对应 .scratch 记录，不渗入选路/展示层。
