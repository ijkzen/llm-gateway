# REQUIREMENTS — 千问 Token Plan（qianwenai）用量查询接入

> 来源请求：「调研 https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1 阿里云千问 token plan 如何获取用量情况，然后为其添加实现」
> 产物：start_work Stage 1（grilling 访谈 → ponytail 削减 → 生产实测确认）

## 背景（事实，均已实测/交叉验证）

- 该 host 是**千问AI平台品牌**的 Token Plan 推理网关：`token-plan.maas.qianwenaiapi.com` 与
  `token-plan.cn-beijing.maas.aliyuncs.com` DNS 解析到同一 NLB（同一后端服务）。
- **推理 Key（`sk-sp-`）不能查用量**（HTTP 200 + `BailianGateway.Login.NotLogined`）；用量走控制台 RPC，需控制台登录态。
- 生产已存在 provider #28 `AliyunTokenPlan`（billing_mode=1、enable=1、`extra` 为空 `{}`）。
- **端到端实测（2026-10-07，用生产 CookieCloud 凭据）**：
  - `GET https://platform-home.qianwenai.com/tool/user/info.json?_t=<ms>`（Cookie）→ `data.secToken`；
  - `POST https://cs-data.qianwenai.com/data/api.json`
    （query：`product=sfm_bailian`/`action=BroadScopeAspnGateway`/`api=<zelda…usage>`/`_t`；
    form：`product`/`action`/`sec_token`/`region=cn-beijing`/`params=<JSON>`/`api`；
    头：`Cookie`/`Origin`/`Referer`/`Accept`/`Cache-Control`/`Pragma`/`Content-Type`）；
  - 响应信封 `code`/`successResponse`/`data.DataV2.data.data`；
  - 实测窗口**仅月窗口**：`per1MonthPercentage: 0.00946406956521739`（= 已用 0.946%）+ `per1MonthResetTime`（ms）——
    控制台已轮换窗口组合（5h/周 → 月），5h/周字段仍可能出现；
  - 比例为 **0–1 已用比例**（官方 `bl` CLI 源码 `×100` 渲染 + 多方抓包确认）；
  - 登录失效 = HTTP 200 + errorCode 标记（`BailianGateway.Login.NotLogined` / `ConsoleNeedLogin` / `NO_LOGIN`）。
- CookieCloud 中 cookie 域名为 `qianwenai.com` → extra 的 `domain` 建议填 `qianwenai.com`。

## 范围

1. 新增 `src/usage/fetchers/qianwenai.rs`：CookieCloud → secToken（JSON 接口）→ 用量 RPC → 解析 → `FetchOutput::Quota`。
2. 解析函数与 CN/INTL Token Plan **共用**（单一事实源，避免两份漂移）。
3. 修复现有 CN/INTL Token Plan 解析（`alibaba.rs`）：①信封路径 `data.DataV2.data.data`（现按 `data.<字段>` 会报"空用量数据"）；
   ②单位 0–1 已用比例（现按 0–100）；③补月窗口解析。
4. 登录失效标记 → `UsageError::Auth` 映射。
5. 分发：`Fetcher::Qianwenai` 变体 + `fetcher_for` **精确 host** 匹配 + 分发测试。
6. 种子模板 `Qianwen Token Plan`（billing_mode=1，extra = CookieCloud 四件套 + `usage`/`usage_type`）+ 存量 provider extra 回填
   （生产 #28 依赖回填才能在 UI 中填写字段：编辑弹窗只渲染 provider 已存 extra 的键）。
7. 测试：解析纯函数单测 + mock 集成测试（成功路径 + 登录失效路径）。
8. 文档：AGENTS.md（fetcher 列表/测试计数）；ADR-0009 如与现状不符则同步。

## 非目标（逐条用户确认）

- 不做 subscription/quota-config 端点（套餐档位、剩余天数）——用户选「只做用量窗口」。
- 不做官方 `bl` CLI 外挂 / AK-SK 换令牌路线——用户选 CookieCloud。
- 不做 qwencloud 国际站 / 其他 qianwenai 变体 host——用户选「仅精确 host」。
- 不做真实凭据的**自动化**测试——用户选 mock + 单测（本次仅人工实测一次，已完成）。
- ponytail 削减：**Cut** `DataV2.data` 内层字符串化容错（实测未出现，出现再补）；
  **Keep** 登录失效标记映射；**Keep** `>1.0 视为百分比` 归一化（1 行，防上游单位漂移误判耗尽/误停用）。
- 不做新 ADR（ADR-0009「新厂商接入 = 新增 fetcher」已覆盖）；CONTEXT.md 无新词汇，不改动。

## 风险与待验证

- CN/INTL 修复无真实账号可实测（依据 CN 百炼公开抓包 + 官方 CLI 证据）；qianwenai 侧已实测跑通。
- 月窗口语义为自然月（非滚动 30 天）；`resets_at` 直接透传，不额外建模。

## 决策记录（grilling 定稿）

| # | 决策 | 选择 |
| --- | --- | --- |
| 1 | 凭据方案 | CookieCloud 控制台 Cookie（沿用现有 Alibaba Token 模式） |
| 2 | Host 范围 | 精确 `token-plan.maas.qianwenaiapi.com` |
| 3 | 验证方式 | 解析单测 + mock 集成测试（另加一次生产实测，已完成） |
| 4 | 展示范围 | 仅用量窗口（5h/周/月，按响应字段防御性填充） |
| 5 | 代码结构 | 新建 `qianwenai.rs`（请求/鉴权流程独立；解析共用） |
| 6 | 模板与回填 | 新增模板 + 存量回填 |
| 7 | CN 修复范围 | 路径 + 单位 + 月窗口一起修 |
