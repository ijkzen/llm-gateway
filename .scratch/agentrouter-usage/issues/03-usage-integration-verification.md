# 03: AgentRouter 用量接口集成验证

**What to build:** 验证端到端真实链路：创建 agentrouter.org host 的 Provider（extra 含 CookieCloud 四键 + `new_api_user`）后，`GET /api/providers/{id}/usage?refresh=1` 能真实抓取并经数据库缓存返回余额形态数据，金额 = quota/500000；mock 上游收到的请求带 `Cookie` 与 `New-Api-User` 请求头。

**Blocked by:** 01 AgentRouter fetcher + host 分发、02 AgentRouter 模板 extra 升级 + 历史回填

**Status:** ready-for-agent

- [ ] provider_usage_integration 新增 agentrouter mock（`LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向）
- [ ] 断言响应 kind=balance、金额 = round2(quota/500000)、label「剩余额度」、currency USD
- [ ] 断言 mock 收到 Cookie + New-Api-User 头
- [ ] 该文件测试与整体 cargo test 全绿
