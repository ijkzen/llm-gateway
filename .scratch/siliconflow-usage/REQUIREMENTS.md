# 硅基流动用量查询 — REQUIREMENTS

来源：用户口头需求（2026-09-07）。start_work 管线 Stage 1 产出。

## 目标（Scope）

为「硅基流动 SiliconFlow（中国站）」Provider 增加用量查询能力：
- 通过其控制台内部钱包接口拉取**余额型钱包**，归一化为 `UsageKind::Balance` 展示在详情页。
- 做成 provider 模板（seed 含凭据结构），历史已存在的 SiliconFlow (China) Provider 通过启动回填补齐凭据键。

## 原始需求（用户逐字）

> 为硅基流动添加用量查询功能，使用
> https://cloud.siliconflow.cn/walletd-server/api/v1/subject/wallets?pageSize=1&stage=3&visible=1&serviceable=1
> 链接去获取相关信息，需要 Cookie 和 X-Subject-Id，其中 Cookie 通过 CookieCloud 获取，
> 然后 X-Subject-ID 需要用户在 Extra 字段里面进行添加，你把它做成模板，历史数据需要回填。

## 关键事实（实测，2026-09-07）

- 端点 `GET https://cloud.siliconflow.cn/walletd-server/api/v1/subject/wallets`
  - 鉴权：`Cookie`（CookieCloud 同步的 `.siliconflow.cn` 登录态）+ `X-Subject-Id`（账号主体 ID，用户手动填 extra）。
  - 实测响应（code 20000）：`data.wallets[]`，每项含
    `walletId / name(多语言JSON) / cap / used / balance / currency(99=CNY) / stage / benefitId /
     availableUses / visible / packageId / notBefore / expiresAt / createdAt / status`，
    外层 `data.pagination.total`、`data.current`。
  - `cap`/`used`/`balance` 为字符串；金额单位 **1 元 = 10¹²**（cap=16000000000000 → 16 元）。
  - 金额字段大数（10¹² 量级），需除 10¹² 转元，浮点转 f64 不丢精度（万亿级 < 2^53）。
  - 实测账号两张钱包：stage=3 认证奖励券（cap=16 元，used=7.18，balance=8.82，expiresAt≈2027-03-09）、stage=4 授信账户 Credit Account（cap=-1 无上限，factor=1，**无 balance 字段**）。stage 1/2/5/6/0 为空。
  - 请求不带 stage 参数时返回全部可见钱包；`cap=-1`（无上限额度）钱包无 balance，不参与余额合计/展示。
  - 服务端要求 `X-Subject-Id`，缺失即失败。
- CookieCloud：域名目标 `cloud.siliconflow.cn`（`fetch_cookies` 后缀匹配 `.siliconflow.cn` 下 cookie）。

## 已拍板决策

| # | 决策 | 结论 |
|---|------|------|
| D1 | 归一化方向 | **Balance**（券余额型钱包），非订阅 Quota |
| D2 | 支持范围 | 仅中国站 `api.siliconflow.cn`（现有「SiliconFlow (China)」模板）；国际站 .com 不做（接口是 .cn 私有控制台） |
| D3 | 券余额门控 | **耗尽则自动停用**（参与按量门控 + LB 排序） |
| D4 | 钱包范围 | **遍历全部余额钱包**（不固定 stage=3；保留 visible/serviceable 过滤语义） |
| D5 | 停用口径 | **余额合计作主**（primary）：所有余额型钱包可用金额合计；明细逐条展示 |
| D6 | 前端 | **维持现状纯数字**（Balance 卡片零改动；不做进度条/到期提示扩展） |
| D7 | primary 实现 | **合并为一条账户余额**（primary）+ 每券一条明细（primary:false） |

## 凭据结构（extra，用户填 + 回填）

SiliconFlow (China) 模板 extra 从 `"{}"` 升级为：

```json
{"cookie_cloud_server": "", "uuid": "", "password": "", "domain": "", "x_subject_id": "", "usage": true, "usage_type": 0}
```

- `cookie_cloud_server` / `uuid` / `password` / `domain`：CookieCloud 凭据（domain 填 `.siliconflow.cn`，实测目标 `cloud.siliconflow.cn`）。
- `x_subject_id`：用户手动填的 X-Subject-Id（**新增键**，历史 provider 缺失需回填为 `""`）。
- `usage: true`（开启用量查询）、`usage_type: 0`（按量余额语义；供后续一致性/展示用）。

### 回填策略（仿 SenseNova 先例）

硅基流动模板**早已 upsert**（base_url host `api.siliconflow.cn`），extra 升级后已存在行走 upsert 的 **update 分支**（不触发模板首次插入回填 `backfill_provider_extra`）。因此：

- 新增启动回填 `backfill_siliconflow_provider_extra`：host 命中 `api.siliconflow.cn` 的历史 provider，extra **只补缺** `cookie_cloud_server/uuid/password/domain/x_subject_id`（置 `""`）+ `usage:true` + `usage_type:0`，不覆盖已设值；复用 `backfill_host_extras` 管线。
- host 判定：`is_siliconflow_host(host)` = host 等于 `api.siliconflow.cn`（仅中国站，D2）。注意 `host_of` 返回小写。

## 实现要点

1. **fetcher**：新增 `src/usage/fetchers/siliconflow.rs`，`fetch_siliconflow_wallets(http, creds)`：
   - `creds.cookiecloud()`（读取 cookie_cloud_server/uuid/password/domain）+ `fetch_cookies` 拼 Cookie 头；
   - `creds.require("x_subject_id")` 取 X-Subject-Id；
   - `GET {API_BASE}?pageSize=50&visible=1`（不带 stage，遍历全部钱包，D4）；
   - 归一化：每张钱包 `cap != -1`（余额型，有 cap 上限）→ `BalanceItem{ label: 券名, amount: balance/1e12, currency: CNY, primary: 参与合计 }`；
   - **授信账户（cap=-1）跳过不展示**；`balance` 缺失/不可解析跳过。
   - 参与 primary 合计：所有余额型钱包的 balance 之和 > 0 → 可用；==0 → 耗尽停用（D3/D5）。实现上给每个余额条目 `primary: true`（后端 `primary_balance()` 找 primary 第一条做 LB 比较，合计 >0 时首条必>0 才…）。⚠️ 见「歧义」。

## ⚠️ 遗留歧义（已问用户？）

- **primary 与「合计」的实现冲突（已拍板，D7）**：现网 `BalanceItem.primary` 是「单条主字段」，`balance_usable()` 取 primary 第一条金额。**合并为一条账户余额**：`BalanceItem{ label: 账户余额, amount: Σ(各余额钱包 balance)/1e12, primary: true }` + 每张券各一条 `primary:false` 明细。门控/LB 取 primary 第一条即合计，共享逻辑零改动。前端 grid 显示多条（账户余额行 + 各券明细行）。

## 非目标（Non-goals）

- 不做国际站（api.siliconflow.com）。
- 不把券映射成订阅额度窗口（不做 Quota）。
- 不做前端扩展（进度条 / 到期提示 / X-Subject-Id 输入 UI 增强）；extra 仍是原生的 JSON 键值编辑。
- 不改授信账户展示（cap=-1 跳过）。
- 不调研 SiliconFlow 是否真会「券耗尽后拦截请求」（用户已选耗尽即停用）。
