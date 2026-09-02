# Spec — SenseNova（商汤）用量查询

标签：`ready-for-agent`

## Problem Statement

用户在网关接入了商汤 SenseNova（Token Plan 订阅制），但供应商详情页没有用量卡片数据：无法看到通用/专属积分池的 5 小时与 7 天窗口剩余额度，订阅额度耗尽时也不会自动停用/恢复。商汤的用量接口不认 `sk-` 密钥，只认控制台 OAuth 体系——需要用长期 refresh_token 自行续期（且 refresh_token 每次刷新轮换、必须写回），这是现有用量 fetcher 体系未覆盖的场景。

## Solution

新增 SenseNova 用量 fetcher：用供应商 `extra.refresh_token` 向商汤 OAuth 端点续期换取 access_token，随即把轮换后的新 refresh_token 写回数据库，再调用 pool-usage 接口取回各积分池用量，逐池产出 5h/7d 用量窗口（带池名标签）。用量卡片展示每池独立的窗口进度条；额度门控与 LB 排序沿用现有逐窗口判定。同时新增 SenseNova 供应商模板；模板首次插入时自动向同 host 的既有供应商补齐缺失的 extra 键，让已建商汤供应商的用户无需手动改配置。

## User Stories

1. 作为管理员，我想在商汤供应商详情页看到用量卡片，以便了解 Token Plan 各积分池的消耗情况。
2. 作为管理员，我想看到每个积分池（通用/专属）独立的 5 小时与 7 天窗口进度条与重置时间，以便精确判断哪个池快耗尽、影响哪些模型。
3. 作为管理员，我只需在创建/编辑商汤供应商时粘贴一次 refresh_token，之后系统自动完成续期与轮换落盘，无需我再碰浏览器。
4. 作为管理员，当 refresh_token 失效（退出登录/轮换断链）时，我希望用量卡片给出明确的鉴权失败提示，以便知道要重新提取 refresh_token。
5. 作为管理员，我希望专属积分池耗尽时整个供应商按现有规则自动停用、恢复后自动启用，与订阅额度门控行为一致。
6. 作为管理员，我希望 LB 订阅制排序能按各窗口剩余比例比较商汤成员，与其他订阅制供应商一致。
7. 作为已有商汤供应商的管理员，我希望功能上线后我的供应商自动补齐用量所需的 extra 字段（不覆盖我已设置的值），以便无需手动编辑 JSON。
8. 作为新建商汤供应商的管理员，我希望按 base_url 自动匹配到 SenseNova 模板并预填用量开关，以便开箱即用。
9. 作为开发者，我希望轮换写回及时（每次成功续期立即落库），以便并发刷新最坏只产生偶发报错而不会弄丢凭据。
10. 作为其他厂商的用户，我希望 QuotaWindow 新增的池名标签对既有厂商零影响，以便既有用量展示与判定完全不变。

## Implementation Decisions

- **凭据存储**：refresh_token 明文存 provider `extra.refresh_token`（沿用 Copilot `oauth_token` 先例）；用户拍板，接受文档安全红线的权衡。
- **续期与轮换写回**：每次 fetch 先 `POST /oauth2/token`（form：`grant_type=refresh_token`、`client_id=nova`）换 access_token；成功后**立即**把响应中的新 refresh_token 写回该 provider 的 extra（本项目首次引入 fetcher 写回凭据机制，写回发生在具备数据库连接的用量查询入口层，fetcher 通过返回值携带轮换后的凭据）。
- **并发口径**：不加锁。写回及时的前提下，并发冲突最坏产生偶发 `invalid_grant` 报错，不会作废凭据（显式记录为 ponytail 简化）。
- **用量查询**：`GET /lite/console/v1/tokenplan/pool-usage`（Bearer access_token）；所有数值为字符串需转数字；`reset_at` 为秒级 Unix 时间戳字符串。
- **多池模型**：QuotaWindow 新增可选 `label` 字段（序列化时缺省省略）；每池独立产出 FiveHour（window_5h）与 Weekly（window_7d）窗口，label = 池名；不走 set_window 去重；plan 取 `plan.name`；不消费 `model_ids` 与 `grant_*` 字段。
- **门控/LB 语义不变**：沿用现有逐窗口判定（任一窗口剩余为 0 即不可用）——用户拍板「任一池耗尽即停用」。
- **host 分发**：`token.sensenova.cn` 与 `platform.sensenova.cn` 均映射到 SenseNova fetcher。
- **模板种子**：新增 SenseNova 模板（推理 base_url `https://token.sensenova.cn/v1`，订阅制，usage 开启，extra 含 refresh_token 推荐空字段）。
- **历史回填**：模板**首次插入**（upsert 中 insert 分支）时，向 base_url host 匹配的既有 provider 的 extra 合并模板 extra 中缺失的键；只补缺、不覆盖既有值；作为 upsert_templates 的通用机制实现，不限商汤。

## Testing Decisions

- 好的测试只测外部行为：给定上游响应形状，断言产出的窗口/凭据写回结果，不测内部解析步骤。
- 四个接缝（全部既有接缝扩展，经用户确认）：
  1. fetcher 解析纯函数单测（先例：各 fetcher 自带 mod tests）——续期响应解析、多池 JSON → 多窗口（含 label、字符串数值、秒级时间戳）。
  2. 用量集成测试（先例：provider_usage_integration，`LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向本地 mock）——续期 → 轮换写回 extra → pool-usage 全链路 + 双 host 分发。
  3. 模板回填单测（先例：provider_template/tests）——首次插入回填、只补缺不覆盖、非首次插入不回填。
  4. 前端用量卡片测试（先例：provider-usage-card.test.tsx）——窗口带 label 时显示池名徽标、列表 key 不撞。

## Out of Scope

- access_token 二级缓存（每次 fetch 直接续期）。
- invalid_grant 专项错误文案映射（沿用现有鉴权失败链路）。
- 池名翻译（上游返回中文名直接展示）。
- 池 → 模型映射（`model_ids` 不消费）。
- 并发刷新锁。
- refresh_token 提取流程的独立使用说明文档。

## Further Notes

- 接口事实来源：`/Users/ijkzen/.zcode/workspace/default/sensenova-pool-usage-guide.md`（2026-09-03 实测）；sk- 密钥不能调用量接口，401 `auth_type_disabled`。
- 需求细化与拍板记录见同目录 `REQUIREMENTS.md`。
- 领域词汇「积分池 (Pool)」「窗口标签 (Window Label)」已记入根 CONTEXT.md。
