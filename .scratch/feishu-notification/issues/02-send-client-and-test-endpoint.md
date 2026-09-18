# 02: 飞书消息发送客户端与测试发送接口

**What to build:** 能真正把一条文本消息发到飞书。实现「取 tenant_access_token」与「发消息」两个调用，以及飞书错误码到中文提示的映射；对外提供 `POST /api/notification/feishu/test`——用请求体里的当前值（appSecret 缺省时回落库中值）发一条测试消息，不落库配置但回写 `last_error` / `last_sent_at`。

**Blocked by:** 01（需要配置行做 appSecret 回落与结果回写）

**Status:** ready-for-agent

- [ ] `src/notification/feishu.rs`：取 token —— `POST {open_base}/open-apis/auth/v3/tenant_access_token/internal`，**每次现取不做进程内缓存**（官方语义：剩余有效期 >30 分钟时返回同一 token）
- [ ] 发文本消息：`POST {open_base}/open-apis/im/v1/messages?receive_id_type={open_id|email}`，`Authorization: Bearer <token>`；**`content` 必须是转义后的 JSON 字符串** `{"text":"…"}`，不是对象——这是最容易写错的一处
- [ ] 错误码 → 中文提示映射：`230027` 应用缺少发送消息权限 / `230006` 机器人能力未启用 / `230013` 接收者不在机器人可用范围 / `230034` 接收者 ID 非法 / 未知码兜底带上原始 `code` 与 `msg`
- [ ] `POST /api/notification/feishu/test`：用**请求体当前值**发送（支持保存前先验证）；`appSecret` 缺省回落库中已存值（支持刷新页面后直接测试）；成功与失败都回写 `last_sent_at` / `last_error`
- [ ] 单测：请求体组装（断言 `content` 是 JSON 字符串）；错误码映射表（含未知码兜底）
- [ ] 集成测试：mock 飞书服务器（经 `LLM_GATEWAY_FEISHU_BASE_URL`）收到消息，断言 `receive_id_type` / `receive_id` / 解出的文本正确；mock 返回 `230027` 时接口返回非 0 `code` + 中文权限提示
