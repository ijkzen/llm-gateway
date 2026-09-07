# 03: TokenRhythm 用量接口集成验证

**What to build:** 验证端到端真实链路：创建 tokenrhythm.studio host 的 Provider（extra 含 CookieCloud 四键）后，`GET /api/providers/{id}/usage?refresh=1` 能真实抓取并经数据库缓存返回余额形态数据，金额 = availableBalanceCny；mock 上游收到的请求带 `Cookie` 与浏览器 `User-Agent`。

**Blocked by:** 01 TokenRhythm fetcher + host 分发、02 TokenRhythm 模板新建 + 历史回填

**Status:** ready-for-agent

- [ ] provider_usage_integration 新增 tokenrhythm mock（CookieCloud `/get/{uuid}` + `/api/wallet/summary`，`LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向）
- [ ] 断言响应 kind=balance、金额 = round2(availableBalanceCny)、label「可用余额」、currency CNY、primary
- [ ] 断言 mock 收到 Cookie（含 tr_session）与浏览器 UA
- [ ] 该文件测试与整体 cargo test 全绿
