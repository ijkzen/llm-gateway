# 千问 Token Plan（qianwenai）用量查询 — SPEC

Feature slug: `qianwenai-usage` · Status: `ready-for-agent`
Source: `.scratch/qianwenai-usage/REQUIREMENTS.md`（grilling 定稿 + 生产 CookieCloud 端到端实测，2026-10-07）

## Problem Statement

管理后台里已有一个指向 `https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1` 的千问 Token Plan 供应商（生产 #28
`AliyunTokenPlan`，订阅制、启用中），转发正常，但**看不到用量**：该 host 未接入用量分发，详情页用量卡报「暂不支持该供应商的用量查询」，
于是额度耗尽不会自动停用、恢复不会自动启用、LB 用量感知排序对该成员失效。而该产品的**推理 Key（`sk-sp-`）无法查询用量**——
控制台网关对推理 Key 返回 HTTP 200 + `BailianGateway.Login.NotLogined`；用量只能凭**控制台登录态**（浏览器 Cookie）经内部 RPC 查询。

同一产品的 CN/INTL 品牌（`token-plan.cn-beijing.maas.aliyuncs.com` 等）虽已接入，但解析按 `data.<字段>` 找窗口、把 0–1 已用比例
当 0–100 百分比——真实信封是 `data.DataV2.data.data`，实际结果是读不到窗口（报「空用量数据」），即使读到也会把「已用 1%」显示成
「已用 0.01%」、把额度耗尽（1.0）读成「已用 1%」，门控形同虚设。

## Solution

为千问 Token Plan（qianwenai 品牌 host）新增用量查询：CookieCloud 同步的 `platform.qianwenai.com` 登录态 Cookie →
`platform-home.qianwenai.com/tool/user/info.json` 取 `secToken` → `cs-data.qianwenai.com/data/api.json` 控制台 RPC 查用量 →
归一化为**订阅制用量窗口**（月 / 周 / 5 小时，按上游实际返回的字段防御性填充）展示在供应商详情页。之后 10 分钟数据库缓存、
`usage_refresh` 定时刷新、订阅额度门控（耗尽自动停用、恢复自动启用）、边界实测探活、LB 用量感知排序（FEFO）全部随既有机制自动生效，
无需额外接线。同时**修复 CN/INTL 同款解析**（信封路径 + 0–1 已用比例 + 月窗口），与新产品共用同一解析函数；并新增种子模板与启动回填，
使历史已创建的该 host 供应商只补缺凭据键即可在 UI 填写、无需重建。

## User Stories

1. 作为管理后台用户，我希望在供应商详情页直接看到千问 Token Plan 的**本月剩余额度**，以便判断这个月还能用多少、要不要提前规划。
2. 作为管理后台用户，我希望用量卡展示的是**月窗口**（当前控制台实际下发的形态），并带重置时间，以便知道额度何时回滚。
3. 作为管理后台用户，我希望上游窗口组合变化时（只发月窗口 / 发 5 小时+周 / 都发）展示都能自动适配，而不是某天突然报错或空白。
4. 作为管理后台用户，我希望用量由 `usage_refresh` 定时任务每 5 分钟自动刷新并落库缓存，以便我无需手动点「刷新」。
5. 作为管理后台用户，我希望用量卡在**控制台登录态失效**时明确提示「登录态已失效（请重新登录并同步 CookieCloud）」，以便我知道该去做什么，而不是只看到一串上游错误码。
6. 作为管理后台用户，我希望额度耗尽时该供应商及其名下虚拟模型自动停用、不再参与选路，以便请求不会打到一个已耗尽的套餐上。
7. 作为管理后台用户，我希望额度重置（或实测确认可用）后自动恢复启用，以便不必手工恢复。
8. 作为管理后台用户，我希望订阅制优先的 LB 策略把该供应商纳入用量感知排序（FEFO 截止链），以便请求优先走额度更宽裕的成员。
9. 作为管理后台用户，我希望剩余额度落在 (0,1)% 边界区时先发一次最小实测请求再决定是否停用，以便不误伤「刚好快用完但还能用」的套餐。
10. 作为管理后台用户，我希望只需填 CookieCloud 的 `server/uuid/password/domain` 四个既有键即可开启用量，不需要安装 CLI、不需要 AK/SK、不需要重新登录网关本身。
11. 作为管理后台用户，我希望**历史上已创建的该 host 供应商**（extra 还是 `{}`）升级后自动补齐凭据键，以便我能在 UI 里直接填写、不必重建供应商。
12. 作为管理后台用户，我希望新建供应商时下拉里能看到 `Qianwen Token Plan` 模板（订阅制 + 正确的凭据字段），以便一次填对。
13. 作为管理后台用户，我希望用量抓取遵循该供应商行上的网络代理设置，以便在受限网络下也能出数。
14. 作为管理后台用户，我希望该用量查询**与推理 Key 无关**（不改动 api_key），以便轮换推理 Key 不影响用量展示。
15. 作为持有 CN 阿里云 Token Plan 的管理后台用户，我希望它的用量终于显示正确数字（正确的信封路径、0–1 已用比例、月窗口），以便展示与门控都能正常工作。
16. 作为维护者，我希望 qianwenai 与 CN/INTL 共用**同一个解析函数**，以便上游信封变化时只改一处。
17. 作为维护者，我希望分发只精确匹配 `token-plan.maas.qianwenaiapi.com`，以便伪装成相似后缀的 host 永远不会被带上凭据请求。
18. 作为维护者，我希望登录失效标记（`BailianGateway.Login.NotLogined` / `ConsoleNeedLogin` / `NO_LOGIN`）归入「登录态失效」错误类，以便错误语义与其它 cookie 类供应商一致。

## Implementation Decisions

- **新增 fetcher（独立文件）**：`src/usage/fetchers/qianwenai.rs`，导出 `fetch_qianwenai(http, creds) -> Result<FetchOutput, UsageError>`。
  请求/鉴权流程自成一格（JSON 取 secToken、不同网关宿主与 cornerstoneParam），故与 CN/INTL 的 `alibaba.rs` 分开；
  **解析共用**（见下）。凭据只用 `Credentials::cookiecloud()`（`cookie_server/uuid/password/domain` 四键），不使用 api_key。
- **请求契约（已实测跑通，逐字实现）**：
  - `GET https://platform-home.qianwenai.com/tool/user/info.json?_t=<毫秒>`，头：`Cookie`（CookieCloud 解出）、
    `Accept: application/json, text/plain, */*`、`Cache-Control: no-cache, no-store, max-age=0`、`Pragma: no-cache`、
    `Referer: https://platform.qianwenai.com/home/billing/subscription/token-plan-individual`；响应 `data.secToken`（缺失/空 → `Auth`，按登录态失效处理）。
  - `POST https://cs-data.qianwenai.com/data/api.json`：
    query `product=sfm_bailian` / `action=BroadScopeAspnGateway` / `api=zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage` / `_t=<毫秒>`（值按 form-urlencoded 编码）；
    form `product` / `action` / `sec_token` / `region=cn-beijing` / `params=<JSON 字符串>` / `api`；
    头 `Cookie` + `Origin`/`Referer: https://platform.qianwenai.com…` + `Content-Type: application/x-www-form-urlencoded` + 缓存三头。
    `params` JSON：`{"Api": <同 api>,"Data":{"cornerstoneParam":{"domain":"platform.qianwenai.com","consoleSite":"QIANWENAI","console":"ONE_CONSOLE","xsp_lang":"zh-CN","protocol":"V2","productCode":"p_efm"}},"V":"1.0"}`。
- **共享解析**：`alibaba.rs` 内新增/改造 `pub(super) fn parse_token_plan_usage(body)`，CN/INTL 的 `fetch_alibaba_token` 与
  `fetch_qianwenai` 共用。契约：
  - 成功判定 `successResponse == true` 或 `code == "200"`；外层 `data` 若为字符串则二次解析（既有行为，保留）。
  - 业务失败：`data.success == false` → `Upstream(200, errorMsg|errorCode)`；响应体**原文**命中登录失效标记
    （`BailianGateway.Login.NotLogined` / `ConsoleNeedLogin` / `NO_LOGIN`，任意字段位置、大小写不敏感包含匹配，
    先于 JSON 解析）→ `UsageError::Auth`。
  - 载荷路径 `data.DataV2.data.data`（**不做**内层字符串化容错——ponytail cut）。
  - 窗口：`per5HourPercentage|per1WeekPercentage|per1MonthPercentage`（+ 各自 `*ResetTime` 毫秒）→ 5 小时 / 周 / 月窗口，
    有则填、无则保持 `available=false`；`used_percent = ratio×100`（`ratio > 1.0` 视为已是百分比直通，最终 clamp 0–100——
    防上游单位漂移导致误判耗尽/误停用）。三窗口全不可用 → `Upstream(200, "上游返回了空用量数据…")`（既有文案）。
  - 归一化 `FetchOutput::Quota { plan: None, windows }`（订阅制；月窗口 `resets_at` 直接透传，不建模自然月语义）。
- **CN/INTL 修复**：`fetch_alibaba_token` 保留站点表（CHINA/INTL）与请求构造，解析改走共享函数。
  **行为变化**：CN/INTL 从「报空用量数据」变为能读出窗口，且剩余百分比口径纠正（此前 0–1 当 0–100）。
- **分发**：`Fetcher::Qianwenai` 变体；`fetcher_for` 精确匹配 `"token-plan.maas.qianwenaiapi.com"`（不加通配后缀）。
- **模板**：seed 新增 `Template { name: "Qianwen Token Plan", base_url: "https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1",
  protocol_type: 0, billing_mode: 1, extra: {"cookie_cloud_server":"","uuid":"","password":"","domain":"","usage":true,"usage_type":1} }`。
  前端模板下拉经 `find_by_domain_all` 自动出现，无需前端改动。
- **回填**：`is_qianwenai_host` + `backfill_qianwenai_provider_extra` 注册进 `upsert_templates`，只补缺不覆盖已设值
  （生产 #28 的 extra 为 `{}`，回填后 UI 才渲染出可填字段；编辑弹窗只渲染 provider 已存 extra 的键）。
- **门控 / 缓存 / LB / 前端：零改动**——订阅制（billing_mode=1）自动参与额度门控、边界探活、FEFO 排序与 10 分钟缓存。
- **错误分类**：CookieCloud 404 → MissingCredential（既有文案）；HTTP 401/403/3xx → Auth（既有 `ensure_not_auth_error`）；
  其余非 200 → Upstream；JSON 非法 → Parse。
- **不做** subscription/quota-config 端点（档位、剩余天数）——范围外（用户确认）。

## Testing Decisions

- 好测试 = 只测**外部行为**（解析正确性 / 分发命中 / 回填只补缺 / HTTP 契约），不测内部实现；解析做成纯函数、夹具驱动。
- **① 解析纯函数单测**（`alibaba.rs` 内，与既有 `token_*` 测试同风格）：
  - 月窗口夹具（**用 2026-10-07 生产实测响应原样**）→ 月窗口 available、`used_percent = 0.95`
    （实测 0.9464…%，构造器按 2 位小数收敛）、`resets_at` 有值；
  - 5 小时+周夹具（旧形态）与混合夹具 → 各自窗口正确；
  - 单位：`1.0` → 100% 已用；`>1.0`（如 42.5）→ 42.5% 直通；
  - 空帧（无窗口字段）→ Upstream；业务错误帧（`success:false`）→ Upstream；
  - 登录失效标记（三种）→ Auth；
  - CN/INTL 回归：同夹具经 `parse_token_plan_usage` 路径可解析（锁定修复不回退）。
- **② fetcher HTTP mock 集成**（`tests/provider_usage_integration/` 新增子模块 + 根文件 `#[path]` 注册）：
  本地 axum mock 提供 `/tool/user/info.json` 与 `/data/api.json`，经 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向；
  创建该 host 的 provider（CookieCloud extra）→ `GET /api/providers/{id}/usage?refresh=1`；
  断言：响应 `kind == "quota"`、月窗口百分比正确；mock 侧断言收到的 form 含 `sec_token`、`region=cn-beijing`、
  `params` 含 `consoleSite=QIANWENAI`，query 含 `action=BroadScopeAspnGateway`。先例：`token_accounts.rs`（AgentRouter / Krill）。
  另覆盖登录失效形态 → 返回 4xx 类错误与登录态文案。
- **③ 分发表单测**（`src/usage/mod.rs::host_dispatch_covers_seed_templates`）：补 `("token-plan.maas.qianwenaiapi.com", true)`
  + 反例（如 `token-plan.maas.qianwenaiapi.com.evil.com`）。
- **④ 模板种子 + 回填单测**（`src/provider_template/tests.rs`）：seed 含 `Qianwen Token Plan`（billing_mode=1、extra 四键 + usage 标记）；
  插入 extra=`{}` 的该 host provider → 回填补齐全部键且不覆盖已设值。
- 每条测试留一个「逻辑坏了必红」的最小断言；不新增测试框架/依赖。

## Out of Scope

- 不做 subscription / quota-config 端点（套餐档位、剩余天数、加油包）。
- 不做 qwencloud 国际站与其它 `*.qianwenai.com` / `maas.*` host 变体（仅精确本 host）。
- 不做官方 `bl` CLI 外挂 / AK-SK 换令牌路线。
- 不做真实凭据的自动化测试（人工实测已完成一次，记录见 Further Notes）。
- 不做 `DataV2.data` 内层字符串化容错（实测未出现；出现再补）。
- 不做前端改动、不做新 ADR、不改 CONTEXT.md（无新词汇；ADR-0009 已覆盖「新厂商接入 = 新增 fetcher」）。

## Further Notes

- **2026-10-07 端到端实测**（生产 CookieCloud 凭据；只验证，不写库）：
  - CookieCloud（`cookie.ijkzen.cn`）含 `qianwenai.com` 域 cookie（18 条）；
  - `user/info.json` → 200，`data.secToken` 存在；
  - 用量 RPC → 200，信封与 CN 百炼同构，载荷仅月窗口：
    ```json
    { "code": "200", "successResponse": true,
      "data": { "success": true, "errorCode": "", "errorMsg": "",
        "api": "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage",
        "DataV2": { "ret": ["SUCCESS::接口调用成功"],
          "data": { "msg": "Success.", "code": "SUCCESS", "success": true,
            "data": { "per1MonthPercentage": 0.00946406956521739,
                      "per1MonthResetTime": 1794067200000 } } } } }
    ```
- 控制台会轮换窗口组合（5h/周 → 月）；官方 `bl` CLI 与多方抓包确认比例为 **0–1 已用**、月窗口为自然月。
- CN/INTL 修复无账号可实测，依据 CN 百炼公开 HAR 抓包 + 官方 CLI issue 证据；qianwenai 侧已实测。
- 上线后生产 #28 需在 UI 填 4 个 CookieCloud 字段（`domain` 建议 `qianwenai.com`）并开启用量开关才会出数（回填负责把字段补出来）。
