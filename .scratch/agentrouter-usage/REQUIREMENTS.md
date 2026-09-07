# AgentRouter 用量查询 — REQUIREMENTS

来源：用户口头需求（2026-09-07），start_work 管线 Stage 1 产出。

## 目标（Scope）

为 AgentRouter（agentrouter.org，QuantumNous/new-api 公益站）Provider 增加用量查询：
- 用 **session Cookie（CookieCloud 同步）+ `New-Api-User` 头** 拉 `/api/user/self` 的 `quota`（剩余额度），归一化为 `UsageKind::Balance` 单条余额展示在详情页。
- 做成 provider 模板（seed extra 升级为凭据结构），历史已存在的 AgentRouter Provider 通过启动回填补齐凭据键。
- 按量门控 / LB 排序随现有 Balance 机制自动生效（billing_mode=0）。

## 原始需求（用户逐字）

> 根据下面的 curl 请求为 agentrouter 添加用量查询功能，历史数据需要回填；
> 其中 Cookie 从 cookiecloud 获取参考已有的 provider；New-Api-User 用户在 extra 手动填写；
> http-proxy 不需要再 extra 中配置，供应商已有网络代理配置；显示额度是 quota 字段除以 5000 显示。

实测 curl（2026-09-07，走本地代理 127.0.0.1:7897）：

```
curl -x http://127.0.0.1:7897 'https://agentrouter.org/api/user/self'
  -H 'Cookie: session=MTc4ODc1NzYyOXx...; acw_tc=0a0f6bdf...'
  -H 'New-Api-User: 591449'
  -H 'User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ... Chrome/152.0.0.0 Safari/537.36'
```

响应 `{"data":{"id":591449,"quota":99998626,"used_quota":1374,"request_count":7,...},"message":"","success":true}`。

## 已拍板决策（grill-with-docs 2026-09-07）

| # | 决策 | 结论 |
|---|------|------|
| D1 | quota 换算 | **÷500000（纠正用户口述的 ÷5000）**。依据：2026-09-05 调研双向对账结论「站点 QuotaPerUnit=50万=$1，与 billing/usage total_usage 美分值吻合」，初始 `quota+used_quota=1e8` ÷500000 = 恰好 $200 整。若 ÷5000 得 ~$20000，量级不符。 |
| D2 | 展示条目 | **只显示剩余一条**：`BalanceItem{ label: 剩余额度, amount: round2(quota/500000), currency: USD, primary: true }`（复用既有 i18n「剩余额度」→ "Remaining Credits"，无需新增映射）。不做已用/总额行。 |
| D3 | extra 键名 | `new_api_user`（snake_case，对齐 x_subject_id / CookieCloud 键风格）。 |
| D4 | 出站 UA | **硬编码浏览器 UA**（fetcher 内 const，与用户 curl 相同）。依据 09-05 调研：agentrouter.org 的 /api/* 在阿里云 WAF 后，脚本 UA 短时多请求触发 JS 挑战（200+HTML）；浏览器 UA 可过。 |
| D5 | 鉴权失败识别 | HTTP 401/403/3xx → `UsageError::Auth`（提示重同步 CookieCloud）；WAF 挑战 HTML（200 但非 JSON）→ 解析失败按 Parse 报错；其余非 200 → Upstream。 |
| D6 | 代理 | 不需要 extra 代理配置：`query_provider_usage` 已统一按 `provider.proxy_enabled/proxy_addr` 创建带代理的 `UsageHttp`（http.rs `with_proxy`），fetcher 无感。 |
| D7 | Cookie 域 | 走标准四键 `cookie_cloud_server/uuid/password/domain`（domain 填 `agentrouter.org`），复用 `Credentials::cookiecloud()` + `fetch_cookies` + `cookie_header`。 |

## 凭据结构（extra，用户填 + 回填）

AgentRouter 模板 extra 从 `"{}"` 升级为：

```json
{"cookie_cloud_server": "", "uuid": "", "password": "", "domain": "", "new_api_user": "", "usage": true, "usage_type": 0}
```

- `cookie_cloud_server/uuid/password/domain`：CookieCloud 凭据（domain = `agentrouter.org`，需同步该域 session 与 acw_tc cookie）。
- `new_api_user`：用户手动填的 New-Api-User 值（**新增键**，历史 provider 缺失需回填为 `""`）。
- `usage: true`（开启用量查询）、`usage_type: 0`（按量余额语义）。

### 回填策略（仿 SenseNova / Krill / SiliconFlow 先例）

AgentRouter 模板**早已 upsert**（seed.rs 现有条目，base_url host `agentrouter.org`，extra=`{}`）。extra 升级后已存在行走 upsert 的 **update 分支**（不触发模板首次插入回填）。因此仿 SiliconFlow：

- 新增启动回填 `backfill_agentrouter_provider_extra`：host 命中 `agentrouter.org` 的历史 provider，extra **只补缺** `cookie_cloud_server/uuid/password/domain/new_api_user`（置 `""`）+ `usage:true` + `usage_type:0`，不覆盖已设值；复用 `backfill_host_extras` 管线。
- host 判定：`is_agentrouter_host(host)` = host 等于 `agentrouter.org`。

## 实现要点

1. **fetcher**：新增 `src/usage/fetchers/agentrouter.rs`：
   - `creds.cookiecloud()` + `fetch_cookies` 拼 Cookie 头；`creds.require("new_api_user")` 取 New-Api-User。
   - `GET https://agentrouter.org/api/user/self`，硬编码浏览器 UA。
   - 归一化：`quota` 存在 → 单条 `BalanceItem{ label: "剩余额度", amount: round2(quota/500000), currency: "USD", primary: true }`。
   - `quota` 缺失 / success!=true → Parse/Upstream 错误。
2. **host 分发**：`fetcher_for("agentrouter.org")` → `Fetcher::AgentRouter`（direct host match，与 siliconflow 同风格）。
3. **模板**：seed.rs AgentRouter 条目 extra 升级 + 注释。
4. **回填**：`provider_template/mod.rs` 加 `is_agentrouter_host` + `backfill_agentrouter_provider_extra`，seed() 里调用。
5. **测试**：fetcher 解析单测（真实响应夹具 / 无 quota / success=false / 401 语义）、host 分发覆盖、seed 模板计数不变（只改 extra 不影响 count）。

## 非目标（Non-goals，ponytail 裁剪）

- 不做 `used_quota`/总额/累计充值行（D2 只显示剩余一条）。
- 不加 `types.rs` i18n 映射（复用「剩余额度」）。
- 不改 `template_default_headers` / 转发层（用量抓取独立路径，浏览器 UA 只在 fetcher 内硬编码；`/v1` 模型接口的 UA 白名单是另一个已搁置问题，见 agentrouter-provider-research 记忆）。
- 不做前端改动（Balance 卡片已支持单条余额展示）。
- 不做代理 extra 键（D6）。
- 不处理 WAF 挑战的自愈（低频请求 + 浏览器 UA 已规避；挑战 HTML 明确报错即可）。
- 不做 New-API 站点的 quota 单位自动发现（固定 500000，站点改配置需人工改 const）。
