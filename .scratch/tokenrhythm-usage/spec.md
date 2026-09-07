# TokenRhythm 用量查询 — SPEC

Feature slug: `tokenrhythm-usage` · Status: `ready-for-agent`
Source: `.scratch/tokenrhythm-usage/REQUIREMENTS.md`（用户口头 + curl 实测，2026-09-07）
规格同构：`.scratch/agentrouter-usage/spec.md`（AgentRouter 用量查询）

## Problem Statement

TokenRhythm（tokenrhythm.studio，「基元律动」自研多模型路由站）是一个已在使用但代码库没有模板的 Provider。它的账户余额不在 OpenAI 兼容的 `/v1/*` 接口里，而需要用户中心登录态的站点内部接口 `GET /api/wallet/summary` 才能查到。用户希望在 Provider 详情页直接看到还能用多少额度（当前主要是赠送券可用余额），且生产已手动创建的历史 Provider 能自动补齐凭据键。

## Solution

为 TokenRhythm 增加用量查询：用量刷新定时任务抓取 `GET https://tokenrhythm.studio/api/wallet/summary`，凭据 = CookieCloud 同步的浏览器登录态 Cookie（`tr_session`/`tr_csrf` 等）+ 硬编码浏览器 UA。把响应的 `data.availableBalanceCny`（可用余额，单位元，字符串）归一化为**单条按量余额**（label「可用余额」，currency CNY），展示在 Provider 详情页用量卡片。余额耗尽时自动停用该 Provider 及其虚拟模型子模型，恢复后自动启用。新建 TokenRhythm provider 模板，生产历史手动 Provider 由每次启动幂等回填自动补齐凭据键。

## User Stories

1. 作为管理后台用户，我希望能查看 TokenRhythm Provider 的可用余额，以便判断还能用多少、是否要充值。
2. 作为管理后台用户，我希望 TokenRhythm 额度显示为**人民币元**（`availableBalanceCny` 直显，站点响应 currency=CNY），与站内钱包数字一致。
3. 作为管理后台用户，我只需在 extra 填 CookieCloud 的 `server/uuid/password/domain` 四个键（domain=`tokenrhythm.studio`），就能开启用量查询——与其它 Cookie 类供应商一致，无需额外身份头。
4. 作为管理后台用户，我不需要在 extra 配代理——供应商行上已有的网络代理设置会自动用于用量抓取。
5. 作为管理后台用户，当可用余额耗尽时该 Provider 及其名下虚拟模型自动停用、不参与 LB；充值/赠送释放后自动恢复。
6. 作为管理后台用户，生产已手动创建的 TokenRhythm Provider（extra 为 `{}` 或缺失键的旧数据）升级后自动补齐凭据键，无需手工重建。
7. 作为运维人员，用量抓取失败能区分「登录态失效（需重同步 CookieCloud）」（401/403/3xx → Auth）与「上游/业务异常」（其余 → Upstream/Parse）。

## Implementation Decisions

- **归一化形态**：`UsageKind::Balance`，单条 `BalanceItem { label: "可用余额", amount: round2(availableBalanceCny), currency: "CNY", primary: true }`。复用既有 i18n 映射「可用余额」→ "Available Balance"。门控 / LB 排序随既有 Balance 机制自动生效（billing_mode=0）。
- **门控口径**：仅 `availableBalanceCny`（可立即花的钱）。`giftLockedCny`（赠送待激活，giftStatus=pending_activation）、recharge/debt/frozen/giftTotal 均不参与。余额→0 停用、>0 恢复。
- **凭据（extra 结构）**：`cookie_cloud_server` / `uuid` / `password` / `domain`（标准 CookieCloud 四键，模板中为空串由用户填；domain 填 `tokenrhythm.studio`）+ `usage: true` + `usage_type: 0`。无额外身份头键。
- **fetcher 端点**：`GET https://tokenrhythm.studio/api/wallet/summary`。请求头：`Cookie`（CookieCloud 解出的 tokenrhythm.studio cookie 拼接）、硬编码浏览器 `User-Agent`。鉴权失败识别：HTTP 401/403/3xx → `UsageError::Auth`；`code != 0` → `Upstream`；`data.availableBalanceCny` 缺失/不可解析 → `Parse`。金额字段为字符串（`9.74899120`），按 f64 解析 + round2。
- **代理**：不新增 extra 键。`query_provider_usage` 已按 `provider.proxy_enabled/proxy_addr` 统一构造带代理的 `UsageHttp`，fetcher 无感。
- **host 分发**：`fetcher_for("tokenrhythm.studio", _)` → `Fetcher::TokenRhythm`（direct host match）。
- **模板**：seed 新建 name=TokenRhythm 条目（alphabet T 段），base_url=`https://tokenrhythm.studio/v1`，protocol_type=0（OpenAI Compat），billing_mode=0（按量），extra = cookie 四键 + usage/usage_type。模板总数 +1。
- **历史回填**：新增每次启动幂等回填 `backfill_tokenrhythm_provider_extra`，host 精确命中 `tokenrhythm.studio` 的历史 provider 只补缺上述键 + `usage:true` + `usage_type:0`，不覆盖已设值；复用既有 `backfill_host_extras` 管线。host 谓词 `is_tokenrhythm_host` 只认 `tokenrhythm.studio`（大小写不敏感，子域不动）。即使新模板首次插入回填（`backfill_provider_extra`）已覆盖历史行，仍每次启动对齐（与 AgentRouter/Krill/SiliconFlow 一致，防模板 update 后漏补）。
- **无前端改动**：ProviderUsageCard 已支持单条余额渲染。

## Testing Decisions

- 好测试 = 测外部行为（解析正确性 / 分发命中 / 回填补缺不覆盖），不测内部实现。解析函数纯函数化，用夹具驱动。
- **fetcher 解析单测**（`src/usage/fetchers/tokenrhythm.rs`）：真实响应夹具 → 单条 primary「可用余额」CNY + round2(availableBalanceCny)；`code!=0` → 错误；`availableBalanceCny` 缺失 → Parse；非 JSON → Parse。仿 agentrouter.rs / balance.rs。
- **host 分发测试**（`src/usage/mod.rs`）：`host_dispatch_covers_seed_templates` 补 `("tokenrhythm.studio", true)` + 反例（如 `sub.tokenrhythm.studio` / `tokenrhythm.studio.evil.com`）。
- **模板/回填单测**（`src/provider_template/tests.rs`）：模板计数 +1；回填幂等测试（历史 extra 缺失键补齐、已设值不覆盖、其它 host 不动）。仿 agentrouter_history_backfill 测试。
- **usage 集成测试**（`tests/provider_usage_integration.rs` 内新增）：本地 axum mock（CookieCloud `/get/{uuid}` + `/api/wallet/summary`），创建 tokenrhythm.studio host provider → `GET /api/providers/{id}/usage?refresh=1` 返回余额形态金额 = availableBalanceCny；断言出站 Cookie 与浏览器 UA。仿 agentrouter 集成测试。

## Out of Scope

- 不做多明细行（giftLocked/giftTotal/recharge/debt/frozen 均不展示）。
- 不新增 i18n 映射（复用「可用余额」）。
- 不改 `/v1` 转发层 / template_default_headers / 前端。
- 不做金额单位换算（CNY 直显）。
- 不做 WAF/反爬自愈重试（低频 + 浏览器 UA 已规避）。
- 生产 CookieCloud 同步域名加 `tokenrhythm.studio` 属运维动作，不在代码范围。

## Further Notes

- tokenrhythm 是自研站（非 New-API），API key 前缀 `sk_`（区别于 new-api 的 `sk-`），`/v1/models` 需要 Bearer。本次只加用量查询（Cookie 登录态），不动 `/v1` 转发。
- CookieCloud 同步域名关键词需覆盖 `tokenrhythm.studio`（抓到 `tr_session` 等 httpOnly cookie）——同 SiliconFlow/AgentRouter 的 CookieCloud 域名坑。
- 生产历史 Provider 由用户手动创建（base_url 含 tokenrhythm.studio），升级部署后回填自动补键，但 CookieCloud 域名同步与 extra 确认属人工步骤。
- 金额 `giftStatus=pending_activation` 表示赠送券待激活；当前可用 9.75 元全为赠送可用，锁定 58 元不计入（D3）。
