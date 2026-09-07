# 01: TokenRhythm fetcher + host 分发

**What to build:** 在 Provider 用量查询管线上识别 TokenRhythm（base_url host `tokenrhythm.studio`）并真实抓取钱包可用余额：用 CookieCloud 同步的登录态 Cookie（`tr_session`/`tr_csrf` 等）+ 硬编码浏览器 UA 访问 `GET https://tokenrhythm.studio/api/wallet/summary`，把响应 `data.availableBalanceCny` 归一化为单条按量余额（label「可用余额」、currency CNY、primary）。余额耗尽/恢复随既有 Balance 门控与 LB 排序自动生效。抓取失败能区分「登录态失效（需重同步 CookieCloud）」（401/403/3xx → Auth）与「上游/业务异常」（其余 → Upstream/Parse）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 新增 TokenRhythm fetcher（CookieCloud 四键必填 + 硬编码浏览器 UA），请求与解析为独立纯函数
- [ ] `fetcher_for` 分发覆盖 `tokenrhythm.studio`（direct match）
- [ ] 解析单测：真实响应 → 单条 primary「可用余额」CNY + round2(availableBalanceCny)；`code!=0` / 字段缺失 / 非 JSON → 对应错误
- [ ] host 分发测试补 `("tokenrhythm.studio", true)` + 反例（如子域/前缀）
- [ ] cargo test 全绿
