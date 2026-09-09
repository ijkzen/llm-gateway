# 06 · usage 厂商抓取层审查

Type: task
Status: open
Blocked by: 01

## Question

对 usage 厂商抓取层做全量审查：`fetchers/` 全目录（agentrouter/alibaba/api_key/balance/cloud_balance/copilot/krill/sensenova/siliconflow/stepfun/tokenrhythm/volcengine/xiaomi）+ `sensenova_login.rs` / `http.rs` / `cookiecloud.rs` / `volcengine_sign.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：各家解析/签名/窗口口径（含 2026-09-07 前后多次修复的教训点不复查已修项）、CookieCloud 解密边界；
- 实现简洁：13 个 fetcher 的重复结构（既有拍板「用量代码不抽重复」——审查只记录结构性风险，不推翻该拍板）、共享 helper 缺失面；
- 测试覆盖：哪些 fetcher 无单测锁定（对照各家解析测试清单）；
- 模块间调用：与 07 持久化/门控、usage_rank、路由用量预估的边界是否合适。

**归位遗留项**：fetcher「会话失效」分类分歧（CookieCloud 族 3xx=过期 vs 共享判定只认 401/403）+ 隐藏登录冷却状态展示——在此给出重估结论。

产出 `.scratch/code-quality-map-2026-09-09/findings/06-usage-fetcher-layer.md`，Answer 给摘要与需拍板问题。
