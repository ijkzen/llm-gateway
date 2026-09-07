# 01: AgentRouter fetcher + host 分发

**What to build:** 在 Provider 用量查询管线上识别 AgentRouter（base_url host `agentrouter.org`）并真实抓取其账户剩余额度：用 CookieCloud 同步的 session Cookie + extra 手填的 `new_api_user` 访问 `GET https://agentrouter.org/api/user/self`（出站硬编码浏览器 UA），把响应 `quota` 换算成美元余额（÷500000），归一化为单条按量余额（label「剩余额度」、currency USD、primary）。配额耗尽/恢复随既有 Balance 门控与 LB 排序自动生效。抓取失败能区分「登录态失效（需重同步 CookieCloud）」（401/403/3xx → Auth）与「上游异常」（其余非 200 → Upstream；WAF 挑战 HTML / success!=true / quota 缺失 → 解析错误）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 新增 AgentRouter fetcher（CookieCloud 四键 + `new_api_user` 必填 + 硬编码浏览器 UA），请求与解析为独立纯函数
- [ ] `fetcher_for` 分发覆盖 `agentrouter.org`（direct match）
- [ ] 解析单测：真实响应 → 单条 primary「剩余额度」USD + round2(quota/500000)；quota 缺失 / success=false → 错误
- [ ] host 分发测试补 `("agentrouter.org", true)` + 反例（如子域/前缀）
- [ ] cargo test 全绿
