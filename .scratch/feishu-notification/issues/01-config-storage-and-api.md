# 01: 通知渠道配置存储与 API

**What to build:** 后端具备持久化飞书通知配置的能力，并可通过 `GET/PUT /api/notification/feishu` 读写。凭据（appId / appSecret / 接收者）以 AES-256-GCM 密文落库，接口对外只返回掩码后的 appSecret。同时落地 `src/notification/` 模块骨架与**共享的飞书 HTTP 客户端构造**（含测试重定向缝），供 02/03 复用。

**Blocked by:** None（可立即开始）

**Status:** ready-for-agent

- [ ] 迁移 27 建 `notification_channel` 表（`channel` 主键 / `config` 密文 / `enable` / `last_error` / `last_sent_at` / `updated_at`）；**编号必须为 27**（14/15 号段是旧方案残留，不可复用）
- [ ] `notification_channel` 实体 + `src/notification/mod.rs` 门面（共享类型、掩码工具、飞书 HTTP 客户端构造与 `LLM_GATEWAY_FEISHU_BASE_URL` 测试重定向、15s 超时）；重定向手法照搬 `src/usage/http.rs` 的 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE`
- [ ] `GET /api/notification/feishu`：未配置返回 `configured:false` 与空字段（**不是 404**）；已配置返回 appId 明文、appSecret 掩码（前 3 后 4）、receiverType / receiver / enable / lastError / lastSentAt
- [ ] `PUT /api/notification/feishu`：`appSecret` 缺省或 `null` 时**保持库中原值不变**（编辑时不必重输凭据）；提供则加密覆盖；**首次创建时 appSecret 必填**
- [ ] 校验：appId 非空、receiverType ∈ {`open_id`,`email`}、receiver 非空；错误消息中文；统一 `Response<T>` 信封
- [ ] 单测：掩码工具；PUT 缺省 appSecret 时库中密文不变
- [ ] 集成测试（新建 `tests/notification_integration.rs`，走 `tests/common::build_authed_app`）：未配置 → `configured:false`；PUT 后 GET 掩码与明文接收者正确；不带 appSecret 的 PUT 不覆盖凭据
