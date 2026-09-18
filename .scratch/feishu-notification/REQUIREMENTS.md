# 需求：飞书机器人消息通知（feishu-notification）

## 来源请求（用户原话）

> 引入 https://github.com/easy-4-rust/oapi-sdk-rust 来实现飞书机器人消息通知，当发生供应商的自动禁用或者自动启用时，发送飞书消息；在设置页面的备份按钮左边，新增一个消息通知按钮。点击按钮后，出现一个弹窗，弹窗页面可以进行飞书消息配置。
>
> 弹窗页面需要具备以下能力：
> 1. 创建飞书机器人：创建机器人分为两种渠道，一种是手动填写 APP ID 和 APP Secret；另一种是根据 SDK 文档进行扫码创建，前端显示二维码。
> 2. 修改机器人信息：如果已经创建好一个机器人，需要提供修改机器人相关信息的能力。
> 3. 测试消息发送：提供一个测试按钮，测试相关发送消息功能是否正常。

## 范围（Scope）

1. **入口**：设置页 `PageHeader` 操作区内、备份按钮**左侧**新增「消息通知」按钮，点击打开通知配置弹窗。
2. **通知配置弹窗**（三项能力）：
   - **创建**：两条路径——① 手动填写 App ID / App Secret（+ 接收者）；② 扫码创建（前端显示二维码，扫码成功后回填凭据）。
   - **修改**：已配置时展示并允许修改 App ID / App Secret / 接收者 / 启用开关。
   - **测试发送**：一个测试按钮，用表单当前值真实发一条测试消息并反馈结果。
3. **后端通知渠道**：新表 + 独立 API，凭据加密落库、对外掩码；飞书 HTTP 客户端（手写 reqwest，见 ADR-0023）。
4. **自动通知**：供应商可用性发生**自动**停用/启用时异步发飞书纯文本消息。
5. **备份集成**：通知渠道配置随现有备份导出/导入（ADR-0017 已同步）。

## 已拍板的决策（grilling + ponytail 结论）

### 客户端实现

- **手写 reqwest，不引入 `oapi-sdk-rust`**（用户原话要求引入 SDK，调研后改判）。实测依据见 ADR-0023：该 SDK 默认 features 编译不过（`events` 模块调 `cipher` 0.4 API，依赖已升 `cbc` 0.2，上游所有分支均未修），且未发布 crates.io、无 tag、单作者 0 star。需要的能力面仅 6 个 HTTP 调用点，`reqwest` 已是直接依赖。
- **`tenant_access_token` 不做进程内缓存**，每次发送现取（官方语义：剩余有效期 >30 分钟时返回同一 token）。
- **不支持 Lark 国际版**：只认 `open.feishu.cn` / `accounts.feishu.cn`，不处理 `tenant_brand=lark` 的域名切换。

### 存储与接口

- **独立表 + 独立 API，不复用 setting 表**。理由：setting 值明文落库且明文进备份，`app_secret` 会泄漏；独立表可用 `crypto::encrypt`（AES-256-GCM，`enc:v1:`）加密存储，接口一律掩码返回（沿用 provider `api_key` 口径）。
- **接收者可配置**：接收者类型（`open_id` / `email`）+ 接收者值。扫码创建时用返回的创建者 `open_id` 自动预填。
- **记录最近一次发送结果**：存 `last_error` + `last_sent_at`，在弹窗展示。理由：连续失败熔断发生在转发路径上，那里没有 cron 日志可看，不落库则完全不可见。
- **备份集成**：写进导出文件；导入侧字段为 `Option`——缺字段（早于本特性的导出文件）时**不触碰**现有配置，出现即整体替换。

### 通知行为

- **触发范围 = 全部自动转换**（5 类）：额度门控自动停用、额度自动恢复、订阅制边界实测停用/恢复、连续失败熔断自动停用、自动恢复探测自动启用。
- **管理员手动启用/停用一律不发**（操作者自己知道）。
- **只在真实转换时发**：`src/availability.rs` 的转换动作返回 `Ok(true)` 才发，幂等空转不发。
- **异步尽力而为**：`tokio::spawn` 发送，绝不阻塞转发路径；失败不重试，只记录 `last_error`。
- **纯文本消息**（飞书 `text` msg_type），多行结构化：事件标题 / 供应商名 / 原因 / 时间。

### 扫码创建交互

- 后端持有**内存注册会话**（`device_code` + `interval` + 到期时间 + 会话 id），前端每 2-3 秒轮询后端拿状态。
- 前端渲染二维码（新增 `qrcode.react` 依赖），内容 = 后端返回的 `verification_uri_complete`。
- 扫码成功后**回填表单**（app_id / app_secret / 接收者 = 创建者 open_id），用户点「保存」才落库——不自动落库，用户可先审阅修改。

## 非目标（Non-goals，ponytail 裁剪 / 用户确认）

- **不引入 `oapi-sdk-rust`**（ADR-0023）。
- **不支持 Lark 国际版**（`tenant_brand=lark` 域名切换砍掉）。
- **不做 `tenant_access_token` 进程内缓存**。
- **不发手动启用/停用通知**。
- **不重试、不排队、不做发送失败补偿**。
- **不做交互卡片**（只发 `text`）。
- **不做多通道抽象**（只有飞书一个渠道，不建通道类型注册表/插件机制）。
- **不实现 `addons` 编码**（gzip + base64url）：默认应用模板已含 `im:message:send_as_bot` 权限与机器人能力。
- **不做扫码注册会话的多会话管理**（单会话，新 `begin` 替换旧的）。
- **不记录通知历史**（只存最近一次失败原因，不做通知流水表）。

## 事实约束（实现前已确认）

### 触发点（全部在现有代码里，均已核实）

| 转换 | 函数 | 调用点 |
| --- | --- | --- |
| 额度门控自动停用 | `availability::disable_for_quota` | `usage/persist.rs:237`（`apply_usage_gate`） |
| 额度门控自动恢复 | `availability::recover_quota` | `usage/persist.rs:235` |
| 边界实测停用 | `availability::disable_for_quota` | `usage/persist.rs:341`（`probe_one_boundary`） |
| 边界实测恢复 | `availability::recover_quota` | `usage/persist.rs:323` |
| 连续失败熔断停用 | `availability::disable_for_failures` | `availability.rs:371`（`on_forward_failure`） |
| 自动恢复探测启用 | `availability::recover_probe` | `failure_recovery.rs:79` |

- 四个转换函数**均返回 `Ok(true)` 仅当确有状态变化**（幂等时 `Ok(false)`）——这是「只发真实转换」的现成契约。
- `disable_for_failures` / `on_forward_failure` 当前**不接收 provider_name**，只有 `request_id`；需要补名称或在通知层按 id 查库。
- `on_forward_failure` 在**转发热路径**上，发送必须异步，不能引入阻塞。

### 飞书 API（实测 + 官方文档核实）

- 取 token：`POST https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal`，body `{"app_id","app_secret"}`，返回 `{"code":0,"tenant_access_token":"t-…","expire":7200}`。
- 发消息：`POST https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={open_id|email}`，头 `Authorization: Bearer <tenant_access_token>`，body `{"receive_id","msg_type":"text","content":"{\"text\":\"…\"}"}`——**`content` 是 JSON 字符串**，不是对象。
- 常见错误码需给可读提示：`230027` 权限不足、`230006` 机器人能力未启用、`230013` 用户不在机器人可用范围、`230034` receive_id 非法。
- 扫码注册：`POST https://accounts.feishu.cn/oauth/v1/app/registration`，form-urlencoded。`action=begin` 实测返回 `{"device_code","user_code","verification_uri","verification_uri_complete","expires_in":3600,"interval":5}`；`action=poll` 成功时**顶层**返回 `client_id` / `client_secret` / `user_info{open_id,tenant_brand}`。
- 注册轮询三个易错点：**HTTP 400 的响应体也要解析**（`authorization_pending`/`slow_down`/`access_denied`/`expired_token` 均以 400 返回）；`slow_down` 时轮询间隔 +5s；`expires_in` 与 `expire_in` 两种字段名都要接受。
- 二维码内容 = `verification_uri_complete`（已含 `user_code`，不要自己拼）。

### 代码库现状

- 迁移编号当前编至 **26**，新迁移为 **27**（注意：生产库 14/15 号段被旧方案占用，不可复用）。
- `crypto::encrypt(&str) -> String` / `decrypt(&str) -> Result<String>` / `decrypt_or_passthrough`，格式 `enc:v1:<base64>`，密钥来自环境变量 `API_KEY_ENCRYPTION_KEY`。
- `reqwest` 0.12（json / rustls-tls / cookies）已是直接依赖；`src/usage/http.rs` 是现有 reqwest 封装（15s 超时、代理感知、`LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 供测试重定向）——通知客户端应复用同等测试缝。
- 前端 `web/package.json` **没有**任何二维码库，需新增 `qrcode.react`。
- 前端弹窗规范：标题栏与底部操作区固定、仅中间内容区滚动（`web/src/components/providers/ProviderSpeedTestDialog.tsx` 是现成范式）。
- i18n 需同时补 `web/src/i18n/locales/zh-CN.ts` 与 `en.ts`。
- 单文件 ≤1000 行；统一响应结构 `Response<T>`（`code`/`msg`/`data`，成功 `code="0"`）；错误消息中文。

## 文档更新（本阶段已落盘）

- `docs/adr/0023-feishu-notification-self-hosted-http.md`（新增）：自研 HTTP 而非引入 SDK 的决策与依据。
- `docs/adr/0017-backup-export-and-restore.md`（更新）：导出清单加入通知渠道配置，并明确可选字段的导入语义。
- `CONTEXT.md`（新增「通知域」节）：通知渠道 / 自动转换通知 / 扫码创建 三个词条。
