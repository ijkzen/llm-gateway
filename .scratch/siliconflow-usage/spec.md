---
Status: ready-for-agent
Slug: siliconflow-usage
---

# Spec — SiliconFlow（硅基流动）用量查询

## Problem Statement

用户在网关接入了硅基流动（SiliconFlow，中国站 `api.siliconflow.cn`），但供应商详情页没有用量卡片数据。硅基流动不提供公开的余额查询 API，其控制台（cloud.siliconflow.cn）内部钱包接口需要浏览器 Cookie（CookieCloud 同步）外加一个账号主体 ID（`X-Subject-Id`，需用户手动提供）才能拉取——这是现有用量 fetcher 体系未覆盖的鉴权组合。已实测接口可用（2026-09-07，用户提供真实凭据）。

## Solution

新增 SiliconFlow（中国站）用量 fetcher：用 CookieCloud 凭据（`cookie_cloud_server/uuid/password/domain`）取回 `.siliconflow.cn` 登录态 Cookie，加上用户手动填在 `extra.x_subject_id` 的账号主体 ID，调用 `cloud.siliconflow.cn` 钱包接口拉取全部余额型钱包，归一化为 `UsageKind::Balance` 展示在详情页；余额合计作为 primary 参与「余额耗尽自动停用/恢复」门控与按量 LB 排序。供应商模板升级含凭据结构；历史已存在的 SiliconFlow (China) 供应商由启动幂等回填补齐缺失键（只补缺、不覆盖）。

## User Stories

1. 作为管理员，我想在硅基流动供应商详情页看到用量卡片，以便了解账户还有多少可用余额。
2. 作为管理员，我只需在创建/编辑硅基流动供应商时填一次 CookieCloud 凭据与 `X-Subject-Id`，之后系统自动从 CookieCloud 取 cookie 查询，无需我再碰浏览器。
3. 作为管理员，我想看到各余额钱包的逐条余额明细（如「认证奖励券」8.82 元），以便知道钱分布在哪。
4. 作为管理员，我希望看到一条「账户余额」合计行，以便一眼知道可用的总金额。
5. 作为管理员，我希望充值产生的余额钱包能自动出现在用量卡片上，无需改代码（遍历全部钱包而非写死单张）。
6. 作为管理员，当 CookieCloud 无登录态、Cookie 失效或 `X-Subject-Id` 未填时，我希望用量卡片给出明确错误提示，以便定位是配置问题还是平台侧问题。
7. 作为管理员，我希望账户可用余额耗尽时该供应商自动停用（连带停用其全部虚拟模型子模型），充值恢复后自动启用，与既有按量门控行为一致。
8. 作为已有硅基流动供应商的管理员，我希望功能上线后我的供应商自动补齐用量所需的 extra 字段（不覆盖我已设置的值），以便无需手动编辑 JSON。
9. 作为新建硅基流动供应商的管理员，我希望按 base_url 自动匹配到升级后的模板并预填用量开关与凭据键位，以便开箱即用。
10. 作为其他厂商的用户，我希望本次改动对既有余额型供应商的展示、门控与 LB 排序零影响，以便不引入回归。

## Implementation Decisions

### 凭据结构（extra）

SiliconFlow (China) 模板的 extra 从 `"{}"` 升级为：

```json
{"cookie_cloud_server":"","uuid":"","password":"","domain":"","x_subject_id":"","usage":true,"usage_type":0}
```

- `cookie_cloud_server/uuid/password/domain`：CookieCloud 凭据，`domain` 填 `.siliconflow.cn`（实测目标 `cloud.siliconflow.cn` 经后缀匹配可见）。
- `x_subject_id`：**用户手动填**的账号主体 ID（`X-Subject-Id` 请求头）。这是「用户需要在 Extra 字段添加」的键。
- `usage:true` 开启用量查询；`usage_type:0` 按量余额语义。

### fetcher

新增 `src/usage/fetchers/siliconflow.rs`，`fetch_siliconflow_wallets(http, creds)`：

- 凭据：`creds.cookiecloud()` 解 CookieCloud 三件套 + domain；`fetch_cookies` 按 `cloud.siliconflow.cn` 过滤后拼 `Cookie` 头；`creds.require("x_subject_id")` 取 `X-Subject-Id` 头。
- 端点：`GET https://cloud.siliconflow.cn/walletd-server/api/v1/subject/wallets?pageSize=50&visible=1`（**不带 stage**——遍历全部可见余额钱包，用户拍板 D4；`serviceable` 不传，实测与默认等价）。
- 鉴权失败识别：HTTP 401/403/3xx → `UsageError::Auth`；`code != 20000` 或缺失 `data.wallets` → `UsageError::Upstream`/`Parse`。
- **金额单位换算**：`cap/used/balance` 为字符串（1 元 = 10¹² 单位，实测 cap=16×10¹²=16 元）。解析成 `f64` 后除 1e12 得元（万亿级 < 2^53，双精度不丢整元）；不可解析的条目跳过。
- **归一化**（用户拍板 D1/D5/D7）：
  - 余额型钱包（`cap` 存在且 `!= -1`，且 `balance` 可解析）→ 每张一条明细 `BalanceItem{ label: 券名, amount: balance/1e12, currency: "CNY", primary: false }`。
  - 各明细金额求和 → 一条 `BalanceItem{ label: "账户余额", amount: 合计/1e12, currency: "CNY", primary: true }`（**置顶**，供 `balance_usable()`/LB 取 primary 第一条即合计）。
  - 无余额型钱包（授信账户等 `cap=-1`、无 `balance` 字段）**跳过不展示**、不参与合计。
  - 明细顺序：账户余额置顶，其后各券按接口返回序。
- 券名：`name` 为多语言 JSON 字符串（如 `{"zh-cn":"认证奖励券","en-us":"..."}`），取 `zh-cn`；解析失败回退 `benefitId`/`walletId`。

### host 分发

`fetcher_for(host, path)` 增加 `"api.siliconflow.cn" => Fetcher::SiliconFlow`（仅中国站，用户拍板 D2）。`is_siliconflow_host`（provider_template）判定 host == `api.siliconflow.cn`，供回填 host 谓词用。

### 模板种子

`seed.rs` 中「SiliconFlow (China)」条目的 extra 升级为上述结构（`usage:true, usage_type:0`）。「SiliconFlow」（国际站 `api.siliconflow.com`）**保持 `"{}"` 不变**（不做国际站，D2）。

### 历史回填

仿 SenseNova 先例（`backfill_sensenova_provider_extra`）：新增 `backfill_siliconflow_provider_extra(db)`，在 `upsert_templates` 中与 Krill/SenseNova 并列调用。对 host 命中 `api.siliconflow.cn` 的历史 provider，**只补缺** `cookie_cloud_server/uuid/password/domain/x_subject_id`（置 `""`）+ `usage:true`（缺则补），**不覆盖已设凭据值**；`usage_type` 为 `billing_mode` 的派生态，每次启动按当前 `billing_mode` 对齐（与 Krill/SenseNova 先例一致）；复用 `backfill_host_extras` 管线（解密失败/非 JSON 仅 warn 跳过，不阻塞其余行）。

> 触发路径：SiliconFlow (China) 模板早已 upsert 进库，extra 升级走 update 分支（不触发模板首次插入回填 `backfill_provider_extra`），因此必须每次启动幂等回填历史 provider（与 SenseNova 完全同因）。

### 门控 / LB / 展示（零共享逻辑改动）

- fetcher 产出 `UsageKind::Balance` 且 primary=合计 → 现有 `apply_usage_gate` 按量分支（`balance_usable()` = primary>0）自动生效：合计=0 停用、>0 恢复；无法判定（无任何余额条目）不动。
- LB 按量排序取 primary 合计金额（`primary_balance()`），零改动。
- 前端 ProviderUsageCard 的 balance grid 逐条展示数字，**零改动**（用户拍板 D6）。暂不展示券的总额/已用/到期日。
- 边界记录（尊重拍板，非缺陷）：该账号授信账户（cap=-1）无余额字段、不参与合计，故实际不会出现「券 0 但合计>0」；若未来出现某无上限额度账户带余额字段，将计入合计（充值制预付费语义下合理）。

## Testing Decisions

- 好的测试只测外部行为：给定上游 JSON 形状，断言归一化出的余额条目/合计/primary，不测内部解析步骤。
- 四个测试接缝（全部既有接缝扩展，用户确认）：
  1. **fetcher 单测**（先例：各 fetcher 自带 `mod tests` 纯函数测试）——钱包 JSON → 归一化：余额型转明细、合计行置顶 primary、授信账户（cap=-1/无 balance）跳过、单位换算、字符串数值解析、多语言 name 取 zh-cn、code!=20000 报错。
  2. **分发映射测试**（先例：`fetcher_for` 的 `host_dispatch_covers_seed_templates`）——`api.siliconflow.cn` → Some；`api.siliconflow.com` / 无关 host → 按现状（.com 无映射 → None）。
  3. **模板种子测试**（先例：`provider_template/tests`）——SiliconFlow (China) 模板 extra 含全部新键 + usage/usage_type；国际站模板不变。
  4. **历史回填测试**（先例：`provider_template/tests` 既有回填测试）——host=api.siliconflow.cn 的历史 provider 补齐缺失键不覆盖已设值；无关 host 不动；解密失败仅跳过。

## Out of Scope

- 国际站 `api.siliconflow.com`（.com 控制台未验证有同款接口）。
- 券映射为订阅额度窗口（不做 Quota）。
- 前端扩展（券的总额/已用/到期日展示、X-Subject-Id 输入 UI、进度条）——extra 仍为原生 JSON 键值编辑。
- 授信账户（无上限额度）的展示与参与。
- SiliconFlow 是否真会在券耗尽后拦截 API 请求的调研（用户已拍板「耗尽即停用」，信任该语义）。
- 钱包接口的用量集成 mock 测试（.cn 私有域，本地 mock 收益低；靠 fetcher 单测 + 真实验证）。

## Further Notes

- 接口事实来源：2026-09-07 用户提供真实 Cookie + X-Subject-Id 实测。
  - `GET /walletd-server/api/v1/subject/wallets`，Header `Cookie` + `X-Subject-Id`。
  - 实测响应：`{"code":20000,"data":{"wallets":[{walletId,name(多语言JSON),cap,used,balance,currency:99,stage,benefitId,expiresAt,...}],"pagination":{"total":N},"current":...}}`。
  - 金额字符串，1 元 = 10¹² 单位；`currency:99` 推断为 CNY。
  - 实测账号：stage=3 认证奖励券（cap 16 元、used 7.18、balance 8.82、expiresAt≈2027-03-09）+ stage=4 授信账户（cap=-1、无 balance、factor=1）。stage 1/2/5/6/0 为空；不带 stage 返回全部可见钱包。
  - `current` 为服务端毫秒时间戳，与真实时间吻合（接口可用性佐证）。
- 需求细化与拍板记录（D1–D7）见同目录 `REQUIREMENTS.md`。
- 领域词汇：复用「余额型 (Balance)」「主余额 (primary)」「CookieCloud 凭据」等既有 CONTEXT.md 词条，无新术语。
