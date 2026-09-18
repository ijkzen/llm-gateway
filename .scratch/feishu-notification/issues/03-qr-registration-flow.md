# 03: 扫码创建 —— 注册会话与 begin/poll 接口

**What to build:** 管理员点「扫码创建」时，后端能发起一次飞书设备码注册会话并给出二维码内容；前端可轮询会话状态，直到成功拿到新建应用的凭据（appId / appSecret / 创建者 open_id）。

**Blocked by:** 01（复用门面里的 HTTP 客户端构造与测试重定向）

**Status:** ready-for-agent

- [ ] `src/notification/register.rs`：`POST {accounts_base}/oauth/v1/app/registration`，form-urlencoded，`action=begin&archetype=PersonalAgent&auth_method=client_secret&request_user_info=open_id`
- [ ] **二维码内容 = 返回的 `verification_uri_complete` 原样**（已含 `user_code`，不要自己拼 `verification_uri` + `user_code`）
- [ ] 注册会话存 `AppState`（**单槽位**，新 begin 替换旧的）：`id` / `device_code` / `interval_secs` / `expires_at` / `last_poll_at` / `last_status`
- [ ] `POST /api/notification/feishu/register` → `{sessionId, qrUrl, expiresIn}`
- [ ] `GET /api/notification/feishu/register/{id}` → `{status: pending|slow_down|success|denied|expired, appId?, appSecret?, receiver?, receiverType?}`；`success` 时 `receiver` = 创建者 `open_id`、`receiverType="open_id"`
- [ ] **三个必须守住的坑**：① HTTP 400 的响应体也要解析（`authorization_pending` / `slow_down` / `access_denied` / `expired_token` 全以 400 返回，把非 2xx 当硬错误会让第一次轮询就杀掉会话）；② `slow_down` 时轮询间隔 +5s；③ `expires_in` 与 `expire_in` 两种字段名都要接受
- [ ] 空 `error` 且无凭据 → 视为 `pending` 继续轮询；`sessionId` 不匹配 / 不存在 / 已过 `expires_at` → `expired`；`denied` / `expired_token` / `success` 后清除会话
- [ ] 后端对飞书的轮询受 `interval` **节流**：距上次真实轮询不足 `interval` 秒直接返回缓存状态（前端 2-3 秒的节奏不会打爆飞书侧）
- [ ] 单测：轮询响应解析表驱动——400 三种错误各自的落点、空 error 无凭据、`expires_in` 与 `expire_in` 都能解析、成功取出 `client_id` / `client_secret` / `user_info.open_id`
- [ ] 集成测试：begin 拿到 qrUrl → 轮询 `pending` → mock 返回成功 → 带回凭据；`interval` 内连续轮询，断言 mock 只被真实轮询一次
