# AgentRouter 用量查询 — SPEC

Feature slug: `agentrouter-usage` · Status: `ready-for-agent`
Source: `.scratch/agentrouter-usage/REQUIREMENTS.md`（用户口头 + curl 实测，2026-09-07）

## Problem Statement

AgentRouter（agentrouter.org，QuantumNous/new-api 公益站）是管理后台中一个已有 Provider 模板，但无法查看账户剩余额度。它的账户额度不在 OpenAI 兼容的 `/v1/*` 接口里，而需要登录态的站点内部接口 `/api/user/self` 才能查到；用普通 API key 查不到。用户希望像其它供应商一样，在 Provider 详情页直接看到还能用多少额度，且历史已创建的 AgentRouter Provider 也要能一键看到（不必重建）。

## Solution

为 AgentRouter 增加用量查询：用量刷新定时任务会抓取 `GET https://agentrouter.org/api/user/self`，凭据 = CookieCloud 同步的浏览器 session Cookie + 用户在 extra 手填的 `New-Api-User` 账户号头。把响应的 `quota`（剩余配额）按站点 `QuotaPerUnit` 换算成美元余额，作为**单条按量余额**（label「剩余额度」，currency USD）展示在 Provider 详情页用量卡片。配额耗尽（换算后为 0）时自动停用该 Provider 及其虚拟模型子模型，充值后自动恢复。所有新增字段由启动幂等回填自动补齐到历史 AgentRouter Provider 的 extra。

## User Stories

1. 作为管理后台用户，我希望能查看 AgentRouter Provider 的剩余额度，以便判断还能用多少、是否要充值。
2. 作为管理后台用户，我希望 AgentRouter 的额度显示为**美元余额**（quota ÷ 500000），因为该站点 `QuotaPerUnit=50万=$1`，与我在站内看到的数额一致。
3. 作为管理后台用户，我只需在 extra 填 CookieCloud 的 `server/uuid/password/domain` 四个既有键 + `new_api_user` 一个账户号键，就能开启用量查询（与硅基流动等 Cookie 类供应商一致）。
4. 作为管理后台用户，我不需要在 extra 配置代理——供应商行上已有的网络代理设置会自动用于用量抓取。
5. 作为管理后台用户，当 AgentRouter 配额耗尽时，我希望该 Provider 及其名下虚拟模型自动停用、不再参与负载均衡；充值后自动恢复。
6. 作为管理后台用户，我希望历史上已创建的 AgentRouter Provider（extra 还是 `{}` 的旧数据）在升级后自动补齐凭据键，无需手工重建。
7. 作为运维人员，我希望用量抓取失败时能明确区分「登录态失效（需重新同步 CookieCloud）」与「上游异常」，便于排查。
8. 作为前端用户，我看到的是 ProviderUsageCard 既有的余额展示样式（无需新增 UI），单条「剩余额度 $x」。

## Implementation Decisions

- **归一化形态**：`UsageKind::Balance`，单条 `BalanceItem { label: "剩余额度", amount: round2(quota/500000), currency: "USD", primary: true }`。复用既有 i18n 映射「剩余额度」→ "Remaining Credits"。门控 / LB 排序随既有 Balance 机制自动生效（模板 billing_mode=0）。
- **quota 换算除数固定 500000**（D1，纠正用户口述 ÷5000）：与 2026-09-05 调研双向对账的站点 `QuotaPerUnit=50万/$1` 一致（`quota+used_quota=1e8` → 恰好 $200 初始额度）。
- **凭据（extra 结构）**：`cookie_cloud_server` / `uuid` / `password` / `domain`（既有四键，均为用户填写的空字符串模板；CookieCloud 同步域名关键词须覆盖 `agentrouter.org` 父域，`domain` 字段填 `agentrouter.org`）+ **新增 `new_api_user`**（用户手填）+ `usage: true` + `usage_type: 0`。与 SiliconFlow/SenseNova 等既有 cookie 类模板一致，模板 seed 不预填域名，由用户填。
- **fetcher 端点**：`GET https://agentrouter.org/api/user/self`。请求头：`Cookie`（CookieCloud 解出的 session/acw_tc cookie 拼接）、`New-Api-User: <extra.new_api_user>`、硬编码浏览器 `User-Agent`（同用户 curl；规避站点背后阿里云 WAF 对脚本 UA 的 JS 挑战）。
- **鉴权失败识别**：HTTP 401/403/3xx → `UsageError::Auth`；HTTP 200 但 body 非合法 JSON（WAF 挑战 HTML / 空）→ `Parse` 错误；`success != true` 或 `quota` 缺失 → `Upstream`/`Parse` 错误。不做 WAF 自愈（低频 + 浏览器 UA 已规避）。
- **代理**：不新增 extra 键。`query_provider_usage` 已按 `provider.proxy_enabled/proxy_addr` 统一构造带代理的 `UsageHttp`，fetcher 无感。
- **host 分发**：`fetcher_for("agentrouter.org", _)` → `Fetcher::AgentRouter`（direct host match，风格同 `api.siliconflow.cn`）。
- **模板**：seed 中既有 AgentRouter 条目（host `agentrouter.org`，billing_mode=0）的 extra 从 `{}` 升级为上述凭据结构；条目数不变。
- **历史回填**：新增启动回填，host 命中 `agentrouter.org` 的历史 provider 只补缺上述键 + `usage:true` + `usage_type:0`，不覆盖已设值；复用既有 `backfill_host_extras` 管线。host 谓词 `is_agentrouter_host` 只认 `agentrouter.org`（不含子域/大小写不敏感）。
- **无前端改动**：ProviderUsageCard 已支持单条余额渲染。

## Testing Decisions

- 好测试 = 测外部行为（解析正确性 / 分发命中 / 回填补缺不覆盖），不测内部实现。解析函数做成纯函数，用夹具驱动。
- **fetcher 解析单测**（`src/usage/fetchers/agentrouter.rs`）：真实响应夹具 → 单条 primary 剩余额度 + USD + round2(quota/500000)；quota 缺失 / success=false → 错误；401/403/3xx → Auth。仿 `siliconflow.rs` / `balance.rs` 风格。
- **host 分发测试**（`src/usage/mod.rs`）：`host_dispatch_covers_seed_templates` 补 `("agentrouter.org", true)` + 一个不匹配反例（如 `agentrouter.org.evil.com`）。
- **模板回填单测**（`src/provider_template/tests.rs`）：插入一个 extra=`{}` 的 AgentRouter host provider → 跑 upsert/backfill → extra 补齐全部键、`usage:true`、已设值不被覆盖。仿现有回填管线测试。
- **usage 集成测试**（`tests/provider_usage_integration.rs` 内新增）：本地 axum mock + `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向，创建 agentrouter.org host 的 provider → `GET /api/providers/{id}/usage?refresh=1` 返回余额形态且金额 = quota/500000；断言 mock 收到的请求含 `Cookie` + `New-Api-User` 头。仿现有 DeepSeek/Krill mock 测试。

## Out of Scope

- 不做 `used_quota` / 总充值等额外展示行（只显示剩余一条）。
- 不新增 i18n 映射（复用「剩余额度」）。
- 不改 `/v1` 转发层的模板默认头 / UA（用量抓取是独立路径；`/v1` 模型接口的 UA 白名单是另一个已搁置问题）。
- 不做前端改动。
- 不做 WAF 挑战自愈 / 重试。
- 不做 quota 单位自动发现（固定 500000；站点改配置需人工改 const）。

## Further Notes

- 用户实测 curl 响应的 `acw_tc` 是阿里云 WAF cookie，需确认 CookieCloud 同步域名关键词覆盖 `agentrouter.org` 父域（否则抓不到挂父域的 cookie——见 SiliconFlow CookieCloud 父域坑）。
- 2026-09-05 曾调研过 AgentRouter 余额走 PAT（`Authorization`）路线，被 WAF 持续拦截而放弃并记入 `agentrouter-provider-research` 记忆；本次是 **session Cookie + `New-Api-User`** 新路线，实测成功（带 acw_tc cookie + 浏览器 UA），旧结论不阻塞本次。
- 生产如已有 AgentRouter Provider，回填后需在 CookieCloud 同步域名中确认 `agentrouter.org`，并在 extra 填 `new_api_user` 才能出数。
