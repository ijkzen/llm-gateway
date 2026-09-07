# TokenRhythm 用量查询 — REQUIREMENTS

来源：用户口头需求（2026-09-07），start_work 管线 Stage 1 产出。规格与 AgentRouter 用量查询（.scratch/agentrouter-usage/）同构。

## 目标（Scope）

为 TokenRhythm（tokenrhythm.studio，「基元律动」自研多模型路由站）Provider 增加用量查询：
- 用 **CookieCloud 同步的登录态 Cookie** 拉 `GET https://tokenrhythm.studio/api/wallet/summary` 的 `data.availableBalanceCny`（可用余额，元），归一化为 `UsageKind::Balance` 单条余额展示在详情页。
- 新建 provider 模板（TokenRhythm，OpenAI Compat，按量），历史已存在的手动 provider 通过每次启动幂等回填补齐凭据键。
- 按量门控 / LB 排序随现有 Balance 机制自动生效（billing_mode=0）。

## 原始需求（用户逐字）

> 按照同样的规格，为 tokenrhythm 添加用量查询功能；[curl 示例]

实测 curl（2026-09-07）：

```
curl 'https://tokenrhythm.studio/api/wallet/summary'
  -H 'Cookie: _c_WBKFRo=...; _nb_ioWEgULi=; tr_session=sess_n9ju-...; tr_csrf=85g1HjHKqulOg-...; tr_ref_device=fnloiTv5kDt1Js9sVj2HcTdycBscahI2'
  -H 'User-Agent: Mozilla/5.0 (Macintosh; ...) Chrome/152.0.0.0 Safari/537.36'
```

响应 `{"code":0,"message":"ok","data":{"currency":"CNY","availableBalanceCny":"9.74899120","giftAvailableCny":"9.74899120","giftLockedCny":"58.00000000","rechargeBalanceCny":"0.00000000","debtBalanceCny":"0.00000000","frozenBalanceCny":"0.00000000","giftTotalCny":"67.74899120","giftStatus":"pending_activation","voidedGiftCny":"0.00000000","asOf":"2026-09-07T06:00:25.806Z"},"traceId":"..."}`。

## 勘察事实（2026-09-07）

- **自研站**（非 New-API）：手机号注册，OpenAI Compat base `https://tokenrhythm.studio/v1`（`/v1/models`、`/v1/chat/completions`、`/v1/embeddings`）+ Anthropic `/v1/messages`；`Authorization: Bearer sk_xxx`。模型含 deepseek-v4-flash 等。用量走用户中心登录态（`/account`），钱包接口 `/api/wallet/summary` 用 Cookie（`tr_session`/`tr_csrf`/`_c_`/`_nb_`/`tr_ref_device`）。
- 代码库零引用：seed.rs 186 条模板无 tokenrhythm；usage fetchers 无此 host。**需从零新建模板**（对比：AgentRouter 是升级既有模板）。
- 生产环境**已有手动创建的 tokenrhythm provider**（用户确认，base_url 含 tokenrhythm.studio），需回填。
- 响应字段：`currency`（CNY）、`availableBalanceCny`=可用余额（当前 9.75）、`giftAvailableCny`=赠送可用（当前=available）、`giftLockedCny`=赠送锁定（58，giftStatus=pending_activation 待激活）、`rechargeBalanceCny`=充值、`debtBalanceCny`=欠费、`frozenBalanceCny`=冻结、`giftTotalCny`=赠送总额（67.75）。金额为字符串、单位元。

## 已拍板决策（grill 2026-09-07）

| # | 决策 | 结论 |
|---|------|------|
| D1 | 模板 | **新建** seed 模板 name=TokenRhythm，base_url=`https://tokenrhythm.studio/v1`，protocol_type=0（OpenAI Compat），billing_mode=0（按量） |
| D2 | 归一化 | **单条 primary**：`BalanceItem{ label: 可用余额, amount: round2(availableBalanceCny), currency: CNY, primary: true }`（复用既有 i18n「可用余额」→ "Available Balance"）。不做多明细。 |
| D3 | 门控口径 | **只看 availableBalanceCny**（可立即花的钱）。giftLocked（待激活赠送）不计入。quota→0 停用、恢复启用，随 Balance 门控/LB 自动生效。 |
| D4 | 回填 | **每次启动幂等对齐**（仿 AgentRouter/Krill 模式）：host 命中 `tokenrhythm.studio` 的历史 provider，extra 只补缺 `cookie_cloud_server/uuid/password/domain` + `usage:true` + `usage_type:0`，不覆盖已设值。即使新模板首次插入回填已覆盖，也每次对齐防模板 update 漏补。 |
| D5 | 凭据 | **标准 CookieCloud 四键** `cookie_cloud_server/uuid/password/domain`（domain=`tokenrhythm.studio`），无额外身份头（curl 只有 Cookie + UA）。 |
| D6 | host 匹配 | 精确 `tokenrhythm.studio`（host_of 提取，大小写不敏感），子域/其它不动。 |
| D7 | 金额单位 | **CNY 直显**（响应已为元，无换算常量）。 |
| D8 | UA/代理 | 出站硬编码浏览器 UA（规避可能的 WAF/反爬，同 agentrouter）；代理复用 provider 行 proxy 配置（query_provider_usage 统一处理，不进 extra）。 |
| D9 | 鉴权失败识别 | 401/403/3xx → Auth；`code!=0` 或 `data.availableBalanceCny` 缺失 → 业务/解析错误。 |

## 实现要点

1. **fetcher**：新增 `src/usage/fetchers/tokenrhythm.rs`：CookieCloud 四键拉 cookie → 拼 Cookie 头 → `GET https://tokenrhythm.studio/api/wallet/summary`（浏览器 UA）→ 解析 `code==0` + `data.availableBalanceCny` → 单条 primary「可用余额」CNY。
2. **host 分发**：`fetcher_for("tokenrhythm.studio")` → `Fetcher::TokenRhythm`。
3. **模板**：seed.rs 新建 TokenRhythm 条目（alphabet 位置，T 段），extra = cookie 四键 + usage 开关。
4. **回填**：`provider_template/mod.rs` 加 `is_tokenrhythm_host` + `backfill_tokenrhythm_provider_extra`，seed() 调用。
5. **测试**：fetcher 解析单测（真实夹具/缺字段/code!=0）、host 分发覆盖、模板计数 +1 与回填幂等测试、集成测试（CookieCloud mock + wallet summary mock）。

## 非目标（Non-goals，ponytail 裁剪）

- 不做多明细行（D2 单条）。不加 giftLocked/giftTotal/recharge/debt/frozen 展示。
- 不抽共享 round2/浏览器 UA（第二轮重复，等第三个先例再抽，避免扩大 diff 波及已提交 agentrouter 文件）。
- 不改前端 / 转发层 / template_default_headers（同 agentrouter，用量走独立路径）。
- 不做 New-Api-User 之类的额外头（该站不需要）。
- 不做金额单位换算（D7）。
- 生产回填后需人工在 CookieCloud 同步域名加 `tokenrhythm.studio` 并确认 extra 补键，本功能只负责代码侧。
