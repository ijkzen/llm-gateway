# REQUIREMENTS — SenseNova（商汤）用量查询

来源：用户请求「根据 sensenova-pool-usage-guide.md 为商汤添加用量查询的能力」；
接口事实以 2026-09-03 实测文档 `/Users/ijkzen/.zcode/workspace/default/sensenova-pool-usage-guide.md` 为准。

## 范围

1. **SenseNova fetcher**（`src/usage/fetchers/sensenova.rs`，host `token.sensenova.cn` / `platform.sensenova.cn` 均映射）
   - 凭据：`extra.refresh_token`（明文，copilot 先例）。
   - 续期：`POST https://platform.sensenova.cn/oauth2/token`（form：`grant_type=refresh_token`、`client_id=nova`、`refresh_token=...`）→ 得 access_token（约 3h）+ **轮换后的新 refresh_token**。
   - **轮换写回**（本项目首次 fetcher 写回凭据）：每次成功刷新立即把新 refresh_token 写回 provider.extra.refresh_token。并发安全性：只要写回及时，并发冲突只会产生偶发 `invalid_grant` 报错，不会弄丢凭据，不加锁。
   - 查询：`GET https://platform.sensenova.cn/lite/console/v1/tokenplan/pool-usage`（Bearer access_token）。
   - 解析：`pools[]` **每池独立**产出 `window_5h`（FiveHour）与 `window_7d`（Weekly）窗口，`label` = 池名；数值均为字符串需转数字；`reset_at` 为秒级 Unix 时间戳字符串；plan = `plan.name`。无月窗。
   - 不消费 `model_ids`、`grant_*` 字段（YAGNI）。
2. **QuotaWindow 加 `label: Option<String>`**（`skip_serializing_if = "Option::is_none"`），其他厂商不填，旧数据/接口完全兼容。前端 `WindowRow` 有 label 时显示池名徽标，列表 key 去重（window+label）。
3. **provider_template 种子**：新增 SenseNova（base_url `https://token.sensenova.cn/v1`，usage=true，extra 含 `refresh_token` 推荐空字段）。
4. **历史数据回填**：模板**首次插入**时，向 base_url host 匹配的既有 provider 合并 extra 中缺失的键（只补缺、不覆盖用户已设值）。作为 upsert_templates 的通用小机制，不限商汤。
5. **门控/LB 不改语义**：沿用现有逐窗口判定（任一窗口剩余为 0 即停用/剔除——用户已拍板「任一池耗尽即停用」）；fetcher 直接产出多窗口，不走 set_window 去重。

## 非目标（ponytail 修剪）

- 不做 access_token 二级缓存（每次 fetch 直接续期）。
- 不做 invalid_grant 专项错误映射（沿用 `UsageError::Auth` 链路）。
- 不翻译池名（上游返回中文名直接展示）。
- 不建 池→模型 映射（`model_ids` 不消费）。
- 不加并发锁（见上，写回及时即可）。

## 用户拍板记录

- 凭据存 extra 字段（非 api_key 加密字段）。
- 多池**每池独立展示**（非求和、非仅通用池）。
- 范围 = fetcher + 模板种子（不含独立使用说明文档）。
- 模型携带池信息 = QuotaWindow 加 label 字段。
- 门控 = 任一池耗尽即停用（沿用现有口径）。
- 补充：历史 provider 自动补齐 extra 字段（回填机制）。
