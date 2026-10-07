# 01: Token Plan 共享解析（含 CN/INTL 修复）+ qianwenai fetcher + 分发

**What to build:** 让 Token Plan 用量解析对**真实信封**生效，并接入千问品牌 host。具体：把 CN/INTL 的 Token Plan 解析提取为共享纯函数并修正——载荷路径 `data.DataV2.data.data`（此前按 `data.<字段>`，真实结果是报「空用量数据」）、比例口径 0–1 **已用**（此前按 0–100）、补月窗口（`per1Month*`，当前控制台实际下发形态），登录失效标记（`BailianGateway.Login.NotLogined` / `ConsoleNeedLogin` / `NO_LOGIN`）归入「登录态失效」错误类。新增 qianwenai fetcher：CookieCloud 四键 → `platform-home.qianwenai.com/tool/user/info.json` 取 `secToken` → `cs-data.qianwenai.com/data/api.json` 用量 RPC（query/form/头逐字按 2026-10-07 生产实测契约），共用上述解析。`fetcher_for` 精确匹配 `token-plan.maas.qianwenaiapi.com` 分发到新 fetcher。交付：CN/INTL 用量数字纠正、新 host 能取到窗口（解析与分发单测锁定）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 共享解析函数：月/周/5 小时三窗口（有则填、无则 available=false）、`ratio×100`（`>1.0` 视为已是百分比直通并 clamp）、信封成功判定（`successResponse`/`code`）、业务错误帧 → Upstream、登录失效标记 → Auth、全窗口缺失 → 空用量数据错误
- [ ] CN/INTL `fetch_alibaba_token` 改用共享解析（行为变化：从报错变为正确读出窗口与剩余百分比）
- [ ] qianwenai fetcher：secToken 接口 + 用量 RPC（query `product/action/api/_t`；form `product/action/sec_token/region/params/api`；头：`Cookie`/`Origin`/`Referer`/`Content-Type`/缓存三头）
- [ ] `Fetcher::Qianwenai` 变体 + `fetcher_for` 精确 host 匹配
- [ ] 解析单测：生产实测响应原样夹具（月窗口 ≈0.946% 已用 + 重置时间）、5 小时+周夹具、混合夹具、`1.0`→100%、`42.5`→42.5%、空帧、业务错误帧、三种登录失效标记
- [ ] 分发单测：`host_dispatch_covers_seed_templates` 补命中 + 反例（evil 后缀）
- [ ] `cargo test --all-targets` 全绿
