# 03: qianwenai 用量端到端集成验证（mock）

**What to build:** 验证端到端真实链路（本地 mock 上游）：创建 `token-plan.maas.qianwenaiapi.com` host 的供应商（extra 含 CookieCloud 四键）后，`GET /api/providers/{id}/usage?refresh=1` 能经 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向到本地 axum mock、抓取并解析为**订阅制窗口**形态（kind=quota、月窗口百分比正确）；mock 侧断言出站请求形状（form 含 `sec_token`/`region=cn-beijing`/`params` 含 `consoleSite=QIANWENAI`，query 含 `action=BroadScopeAspnGateway`）；并覆盖登录失效形态（HTTP 200 + 登录失效标记 → 错误信息为登录态文案）。

**Blocked by:** 01 Token Plan 共享解析（含 CN/INTL 修复）+ qianwenai fetcher + 分发、02 Qianwen Token Plan 模板种子 + 存量回填

**Status:** ready-for-agent

- [ ] `tests/provider_usage_integration/` 新增子模块 + 根文件 `#[path]` 注册（含 mock：`/tool/user/info.json` + `/data/api.json`）
- [ ] 成功路径：`kind=quota`、月窗口百分比与重置时间正确
- [ ] 出站断言：form `sec_token`/`region`/`params.consoleSite=QIANWENAI`、query `action=BroadScopeAspnGateway`、`Cookie` 头存在
- [ ] 登录失效路径：响应为错误且信息含登录态提示
- [ ] 该文件测试与整体 `cargo test --all-targets` 全绿
