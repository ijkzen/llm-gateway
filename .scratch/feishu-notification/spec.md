# Spec: 飞书机器人消息通知（feishu-notification）

Status: ready-for-agent

## Problem Statement

供应商被网关**自动**停用或自动启用时，管理员完全不知情：额度耗尽的停用、边界实测的停用、连续失败熔断的停用、以及额度恢复/探测成功的自动启用，都只写进数据库和日志。管理员要么定期翻供应商列表，要么等请求失败才后知后觉。需求：这些自动状态变化发生时主动推送飞书消息，并在设置页提供一个配置飞书机器人的入口。

## Solution

设置页 `PageHeader` 的备份按钮**左侧**新增「消息通知」按钮，点击打开通知配置弹窗。弹窗承载三件事：

1. **创建飞书机器人**——两条路径：① 手动填写 App ID / App Secret（+ 接收者）；② 扫码创建：前端显示二维码，管理员用飞书客户端扫码确认后自动取得凭据并回填表单。
2. **修改机器人信息**——已配置时展示 App ID、掩码后的 App Secret、接收者与启用开关，可修改（Secret 留空表示不改）。
3. **测试消息发送**——一个测试按钮，用表单当前值真实发一条测试消息并反馈结果。

配置保存后，供应商发生**自动**停用/自动启用时，网关异步发一条飞书纯文本消息到配置的接收者。

## User Stories

1. 作为管理员，我想在设置页备份按钮左侧看到「消息通知」入口，以便不离开设置页即可配置通知。
2. 作为管理员，我想手动填写 App ID 与 App Secret 创建飞书机器人，以便复用我已在飞书开放平台建好的应用。
3. 作为管理员，我想点「扫码创建」看到二维码，用飞书客户端扫一下就在自己租户里建好应用并自动拿到凭据，以便不手工去开放平台建应用、拷凭据。
4. 作为管理员，我想在扫码成功后看到表单被自动填好（App ID / App Secret / 接收者 = 我的 open_id），以便先确认再保存，而不是被自动落库。
5. 作为管理员，我想在扫码过程中看到当前状态（等待扫码 / 已扫码待确认 / 成功 / 已过期），以便知道还要不要继续等。
6. 作为管理员，我想在二维码过期后能重新发起一次扫码，以便中断后重来。
7. 作为管理员，我想配置接收者（open_id 或邮箱），以便消息发到我指定的账号。
8. 作为管理员，我想在已配置时看到掩码后的 App Secret，以便确认配置存在而不泄露完整凭据。
9. 作为管理员，我想修改已配置的机器人的 App ID / 接收者 / 启用开关，而不必重新输入 App Secret，以便改接收者这种小改动不折腾。
10. 作为管理员，我想在需要时替换 App Secret，以便轮换凭据。
11. 作为管理员，我想有一个启用/停用开关，以便临时静音通知而不删掉配置。
12. 作为管理员，我想点「测试发送」立刻验证配置是否可用，以便在真正出事之前知道消息能不能发出去。
13. 作为管理员，我想测试发送失败时看到具体原因（权限不足 / 机器人能力未启用 / 用户不在可用范围 / receive_id 非法），以便知道去开放平台改什么。
14. 作为管理员，我想在配置后看到最近一次发送结果（成功时间或失败原因），以便在通知悄悄失败时能发现。
15. 作为管理员，供应商因**额度耗尽**被自动停用时我想收到飞书消息，以便及时充值或调整。
16. 作为管理员，供应商因**订阅窗口剩余逼近耗尽、边界实测失败**被自动停用时我想收到飞书消息，以便知道这是实测判定而非额度硬耗尽。
17. 作为管理员，供应商因**连续失败熔断**被自动停用时我想收到飞书消息，以便排查上游故障。
18. 作为管理员，供应商**额度恢复**被自动启用时我想收到飞书消息，以便知道服务已自愈。
19. 作为管理员，供应商**自动恢复探测成功**被启用时我想收到飞书消息，以便知道上游已恢复。
20. 作为管理员，我**手动**停用/启用供应商时**不要**收到飞书消息，因为是我自己刚点的。
21. 作为管理员，我想消息内容包含事件类型、供应商名、原因与时间，以便一眼看懂发生了什么。
22. 作为管理员，我不想通知发送拖慢网关转发（尤其是转发路径上的熔断），以便上游故障时网关仍然快。
23. 作为管理员，我不想通知发送失败反复重试刷屏，以便飞书侧抖动不会造成影响。
24. 作为管理员，我想在备份导出里包含通知配置（含明文 App Secret），以便换机器恢复后不用重新配机器人。
25. 作为管理员，我想导入一个**早于本特性**的旧备份文件时不要清掉我现有的通知配置，以便老备份仍然安全可用。
26. 作为管理员，我不想为了用通知而新增任何外部服务依赖，以便网关保持单二进制部署。

## Implementation Decisions

### 模块划分（后端）

- 新增 `src/notification/` 目录（facade + 子模块，沿用 `src/proxy/`、`src/usage/` 的形态）：
  - `mod.rs`：门面（子模块声明 + `pub use` 重导出）、共享类型、HTTP 客户端构造与测试重定向。
  - `feishu.rs`：飞书 Open API 客户端——取 `tenant_access_token`、发文本消息、错误码 → 中文提示映射。
  - `register.rs`：扫码创建的 device-authorization 流程（begin / poll）与内存注册会话。
  - `notify.rs`：通知触发入口——读配置、拼文案、发送、回写结果。
- 新增 `src/routes/notification.rs`，挂载为 `.nest("/api/notification", notification::routes())`。
- 新增实体 `src/entity/notification_channel.rs`。

### 数据表（迁移 27）

新表 `notification_channel`：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `channel` | TEXT PRIMARY KEY | 渠道标识，当前恒为 `feishu` |
| `config` | TEXT NOT NULL | **AES-256-GCM 密文**（`enc:v1:` 前缀，`crypto::encrypt`），明文为 JSON `{appId, appSecret, receiverType, receiver}` |
| `enable` | BOOLEAN NOT NULL DEFAULT 1 | 通知总开关 |
| `last_error` | TEXT NOT NULL DEFAULT '' | 最近一次发送失败原因（空 = 上次成功） |
| `last_sent_at` | DATETIME NULL | 最近一次发送时间 |
| `updated_at` | DATETIME NOT NULL | |

- 迁移编号必须为 **27**（当前编至 26；14/15 号段被旧方案占用，不可复用）。
- 单行表：同一时刻只有一个飞书渠道，不建通道类型注册表、不做多通道抽象。
- 凭据加密落库的写法与 provider `api_key`/`extra` 同源（ADR-0002/0018 不变式）。

### 飞书 API 契约（实测 + 官方文档核实）

**取 token**：`POST {open_base}/open-apis/auth/v3/tenant_access_token/internal`，JSON body `{"app_id","app_secret"}`，返回 `{"code":0,"msg":"ok","tenant_access_token":"t-…","expire":7200}`。**不做进程内缓存，每次发送现取**（官方语义：剩余有效期 >30 分钟时返回同一 token）。

**发消息**：`POST {open_base}/open-apis/im/v1/messages?receive_id_type={open_id|email}`，头 `Authorization: Bearer <tenant_access_token>`、`Content-Type: application/json`，body：

```json
{"receive_id":"ou_xxx","msg_type":"text","content":"{\"text\":\"消息正文\"}"}
```

`content` 是 **JSON 字符串**（不是对象），这是最容易写错的一处。成功返回 `{"code":0,"msg":"success","data":{"message_id":"om_…"}}`。

**错误码 → 中文提示**（至少覆盖）：

| 码 | 提示 |
| --- | --- |
| `230027` | 应用缺少发送消息权限（`im:message:send_as_bot`） |
| `230006` | 机器人能力未启用 |
| `230013` | 接收者不在机器人的可用范围内 |
| `230034` | 接收者 ID 非法 |
| 其他 | 带上飞书返回的 `code` 与 `msg` |

**扫码注册**：`POST {accounts_base}/oauth/v1/app/registration`，`Content-Type: application/x-www-form-urlencoded`。

- `action=begin&archetype=PersonalAgent&auth_method=client_secret&request_user_info=open_id` → 实测返回 `{"device_code":"v1:…","user_code":"JRDZ-2HEL","verification_uri":"https://open.feishu.cn/page/launcher","verification_uri_complete":"https://open.feishu.cn/page/launcher?user_code=JRDZ-2HEL","expires_in":3600,"interval":5}`。
- **二维码内容 = `verification_uri_complete` 原样**（已含 `user_code`，不要自己拼 `verification_uri` + `user_code`）。
- `action=poll&device_code=…` → 成功时**顶层**返回 `client_id` / `client_secret` / `user_info{open_id,tenant_brand}`。
- **三个必须守住的坑**：
  1. **HTTP 400 的响应体也要解析**——`authorization_pending` / `slow_down` / `access_denied` / `expired_token` 全部以 400 返回，把非 2xx 当硬错误会让第一次轮询就杀掉整个会话。
  2. `slow_down` 时轮询间隔 **+5 秒**。
  3. `expires_in` 与 `expire_in` **两种字段名都要接受**（不同 SDK 版本口径不一）。
- 空 `error` 且无凭据 → 视为 `pending` 继续轮询。
- **只支持飞书国内版**：`open_base = https://open.feishu.cn`、`accounts_base = https://accounts.feishu.cn`，不处理 `tenant_brand=lark` 的域名切换（ADR-0023）。
- **不实现 `addons` 编码**（gzip + base64url）：默认应用模板已含 `im:message:send_as_bot` 权限与机器人能力。

**测试重定向**：客户端构造时读环境变量 `LLM_GATEWAY_FEISHU_BASE_URL`，非空时**同时**替换 `open_base` 与 `accounts_base`（本地 mock 服务器按路径区分：`/open-apis/...` 与 `/oauth/v1/app/registration`）。手法照搬 `src/usage/http.rs` 的 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE`。请求超时 15 秒。

### API 路由契约

统一走 `Response<T>`（`code`/`msg`/`data`，成功 `code="0"`），错误消息中文。

- `GET /api/notification/feishu` → 渠道配置（`appSecret` 掩码）。
  ```
  data: { configured: bool, appId, appSecretMasked, receiverType: "open_id"|"email",
          receiver, enable: bool, lastError, lastSentAt: string|null }
  ```
  未配置时返回 `configured:false` 与空字段（**不是 404**——「尚未配置」是正常状态而非错误）。
  `appSecretMasked` 沿用「key 掩码」口径（保留前 3 后 4，中间 `****`）。
- `PUT /api/notification/feishu` → 保存配置。body `{appId, appSecret?, receiverType, receiver, enable}`。
  - **`appSecret` 为可选**：缺省/`null` 表示**保持库中原值不变**（编辑时不必重新输入凭据）；提供则加密覆盖。写法照搬 provider 更新路由对 `extra` 的处理。
  - 校验：`appId` 非空；`receiverType` ∈ {`open_id`,`email`}；`receiver` 非空；**首次创建时** `appSecret` 必填。
- `POST /api/notification/feishu/test` → 用**请求体里的当前值**发一条测试消息，**不落库**（支持「保存前先验证」）。body `{appId, appSecret?, receiverType, receiver}`；`appSecret` 缺省时回落到库中已存值（支持「刷新页面后直接测试」）。发送失败返回非 0 `code` + 映射后的中文消息。
- `POST /api/notification/feishu/register` → 发起扫码注册。`data: {sessionId, qrUrl, expiresIn}`。
- `GET /api/notification/feishu/register/{session_id}` → 轮询注册状态。`data: {status: "pending"|"slow_down"|"success"|"denied"|"expired", appId?, appSecret?, receiver?, receiverType?}`；`success` 时带回 `appId` / `appSecret` / `receiver`（= 创建者 `open_id`，`receiverType="open_id"`）。

**关于 `appSecret` 明文出参的例外**：`GET` 配置一律掩码，但注册成功这一次会把新建应用的 `appSecret` 明文返回给前端——这是「回填表单、由用户确认后再保存」流程的必要代价（凭据刚由管理员本人扫码创建，且经会话鉴权）。

### 内存注册会话

- 存放在 `AppState`（新增字段），单会话槽位：新 `begin` 直接替换旧的。
- 字段：`id`（uuid）、`device_code`、`interval_secs`、`expires_at`、`last_poll_at`、`last_status`。
- 前端轮询到后端时，**后端对飞书的轮询受 `interval` 节流**：距上次真实轮询不足 `interval` 秒则直接返回缓存状态，避免前端 2-3 秒的节奏把飞书侧打爆（飞书建议间隔 5 秒）。
- `sessionId` 不匹配 / 会话不存在 / 已过 `expires_at` → `status: "expired"`；进入终态（`success` / `denied` / `expired_token`）后**保留**会话与结果直到过期或被新的 `begin` 替换——重复轮询必须返回同一个终态，否则前端第二次轮询会从 `success` 翻成 `expired` 而丢掉刚拿到的凭据。

### 通知触发（关键接线）

- **触发点收敛在 `src/availability.rs` 的四个转换动作内部**，而不是散在调用方：`disable_for_quota`、`recover_quota`、`disable_for_failures`、`recover_probe` 在**确有转换**（即原本返回 `Ok(true)` 的分支）时触发通知。这样未来新增调用方不会漏发。
- 覆盖的 5 类自动转换与既有调用点：

  | 转换 | 动作 | 调用点 |
  | --- | --- | --- |
  | 额度门控自动停用 | `disable_for_quota` | `usage/persist.rs`（`apply_usage_gate`） |
  | 额度门控自动恢复 | `recover_quota` | `usage/persist.rs` |
  | 边界实测停用 | `disable_for_quota` | `usage/persist.rs`（`probe_one_boundary`） |
  | 边界实测恢复 | `recover_quota` | `usage/persist.rs` |
  | 连续失败熔断停用 | `disable_for_failures` | `availability.rs`（`on_forward_failure`） |
  | 自动恢复探测启用 | `recover_probe` | `failure_recovery.rs` |

- **手动启用/停用（`disable_manual` / `enable_manual`）不触发**——操作者自己知道。
- 入口形态：`notification::spawn_provider_transition(db, provider_id, transition)`——**同步函数**，内部 `tokio::spawn` 异步任务。理由：`on_forward_failure` 在**转发热路径**上，绝不能因飞书 HTTP 调用而阻塞。
- `Transition` 枚举承载已本地化的原因：
  ```
  QuotaDisabled { label }      // label = 订阅额度 / 余额
  QuotaRecovered { label }
  FailureDisabled { consecutive }
  FailureRecovered
  ```
- 异步任务内部：读通知配置（单行查询，无缓存 → 无失效 bug）→ 未配置或已停用则直接返回 → 按 `provider_id` 查供应商名（已删除则跳过）→ 拼文案 → 取 token → 发消息 → 回写 `last_error` / `last_sent_at`。
- `disable_for_failures` / `on_forward_failure` 当前**不接收供应商名**（只有 `provider_id` 与 `request_id`）——由异步任务按 id 查名，**不改动这四个函数的签名**。
- **不重试、不排队、不做发送失败补偿**；失败只写 `last_error` + tracing 日志。

**消息文案**（纯文本，`msg_type: "text"`）：

```
【供应商自动停用】
供应商：{name}
原因：订阅额度耗尽
时间：2026-09-18 11:30:00
```

- 标题：`【供应商自动停用】` / `【供应商自动启用】`。
- 原因：`订阅额度耗尽` / `余额耗尽` / `连续失败 {n} 次` / `订阅额度恢复` / `余额恢复` / `自动恢复探测成功`。
- 时间按**设置表时区**（`app_settings` 的 `timezone`，缺省 `Asia/Shanghai`）格式化——与 cron/统计口径一致。

### 备份集成

- `BackupFile` 新增可选字段 `notificationChannels: Option<Vec<…>>`（`#[serde(default)]`）。
- **导出**：恒产出该字段（可能为空数组），`appSecret` 为**明文**（与其他凭据一致——备份必须能脱离原库密钥恢复，ADR-0017）。
- **导入**：`Some(list)` → 先清空 `notification_channel` 再插入（密文重新加密落库）；**`None`（早于本特性的导出文件缺该字段）→ 完全不触碰现有配置**。这是与「整体替换」的唯一例外，理由是避免老备份静默清掉配置。
- 校验：`channel` 非空、`receiverType` ∈ {`open_id`,`email`}、`receiver` 非空、`appId` 非空。

### 前端

- **入口**：`web/src/pages/settings.tsx` 的 `PageHeader` 操作区内、备份按钮**左侧**新增按钮（`Bell` 图标 + 「消息通知」），点击打开通知弹窗。
- **`web/src/components/settings/NotificationDialog.tsx`**：
  - 弹窗结构遵守仓库规范：**标题栏与底部操作区固定、仅中间内容区滚动**（照 `ProviderSpeedTestDialog`）。
  - **未配置态**：并排/分区的两条创建路径——「手动填写」（App ID / App Secret / 接收者类型 / 接收者）与「扫码创建」（一个按钮，点击后显示二维码 + 状态文案 + 过期后「重新获取」）。
  - **已配置态**：只读展示 App ID、掩码 App Secret、接收者、启用开关、最近一次发送结果；「修改」进入编辑态（Secret 输入框留空 = 不改，占位文案说明）；「停用/启用」开关直接生效。
  - **测试发送**：底部操作区左侧按钮，用**表单当前值**调用 test 接口（编辑态未保存也能测）；成功 toast，失败 toast 展示后端返回的中文原因。
  - 扫码成功 → 回填表单并切到编辑态 + toast 提示（**不自动保存**）。
- **`web/src/lib/notification.ts`**：API 模块（照 `lib/backup.ts` 形态）。
- **hooks**：`use-notification.ts`——配置查询/保存/测试/发起注册/轮询状态（轮询用 TanStack Query 的 `refetchInterval`，拿到终态即停）。
- **i18n**：`zh-CN.ts` / `en.ts` 新增 `notification` 命名空间（入口按钮、弹窗标题、两条创建路径、状态文案、测试发送、成功/失败提示、编辑态占位说明、备份相关不变）。
- **新依赖**：仅新增 `qrcode.react`（v4.2.0，peer 支持 React 19），用其 `QRCodeSVG` 渲染二维码；不引入其他前端依赖。

## Testing Decisions

好测试 = 只测外部行为、不测实现细节；优先复用仓库既有缝隙，不新造测试基建。

- **缝 A：后端集成测试**（新增 `tests/notification_integration.rs`，走 `tests/common::build_authed_app`，内存库 + 现有注入凭证 + 本地 mock 飞书服务器，经 `LLM_GATEWAY_FEISHU_BASE_URL` 重定向）：
  - 配置 CRUD：未配置 → `configured:false`；PUT 后 GET 返回 `appId` 明文、`appSecretMasked` 掩码、接收者明文。
  - 编辑不覆盖凭据：PUT 不带 `appSecret` 后再测试发送，断言 mock 收到的是**原** secret。
  - 测试发送：mock 收到消息，断言 `receive_id_type`、`receive_id`、`content` 解出的文本正确。
  - 失败映射：mock 返回 `code:230027` → 接口返回非 0 `code` 且消息为中文权限提示。
  - 扫码注册：`begin` 拿到 `qrUrl`；mock 依次返回 `authorization_pending` → 成功，断言状态流转与最终带回的 `appId`/`appSecret`/`receiver`。
  - 轮询节流：`interval` 内连续轮询前端，断言 mock 只被真实轮询一次。
  - **触发接线（核心）**：配置通知 + mock，走既有用量刷新路径真实触发一次额度门控停用，断言 mock 收到自动停用消息（**有界轮询等待**，非固定 sleep）。
  - **手动不发**：手动停用一个供应商后再触发一次自动转换，断言 mock **恰好只收到 1 条**（自动那条）。
  - 备份：导出含通知配置且 `appSecret` 为明文；导入缺该字段的旧文件 → 现有配置**不被清掉**；导入含该字段 → 整体替换。
- **缝 B：后端纯逻辑单测**（`src/notification/` 内 `#[cfg(test)]`）：
  - 注册轮询响应解析表驱动：**400 + `authorization_pending`/`slow_down`/`access_denied`/`expired_token`** 各自的落点；`slow_down` 间隔 +5；空 `error` 无凭据 → pending；`expires_in` 与 `expire_in` 两种字段名都能解析；成功响应取出 `client_id`/`client_secret`/`user_info.open_id`。
  - 错误码 → 中文提示映射（含未知码兜底带原始 `code`/`msg`）。
  - 发消息请求体组装：`content` 必须是**转义后的 JSON 字符串**。
  - 四类转换 → 文案渲染（标题/原因/时间格式）。
  - 通知主流程（`await` 直接调用、不 spawn，确定性）对 mock：成功后写 `last_sent_at`、失败后写 `last_error`。
- **缝 C：前端组件测试**（`web/src/components/__tests__/notification-dialog.test.tsx`，mock API 模块，沿用现有 dialog 测试的 `act` + mock 模式）：
  - 未配置态展示两条创建路径；手动保存 → 断言 PUT 请求体。
  - 扫码：点击 → 断言渲染二维码 → 轮询 `pending` → `success` → 表单被回填（fake timers 驱动 `refetchInterval`）。
  - 已配置态：掩码展示；Secret 留空提交 → 断言请求体不带 `appSecret`。
  - 测试发送：成功 toast；失败展示后端中文消息。
  - 设置页回归（扩展 `web/src/__tests__/settings-page.test.tsx`）：断言「消息通知」按钮存在且位于备份按钮**左侧**（DOM 顺序）。

## Out of Scope

- **不引入 `oapi-sdk-rust`**（ADR-0023：其默认 features 编译不过、未发布 crates.io、单作者 0 star；需要的能力面仅 6 个 HTTP 调用点）。
- **不支持 Lark 国际版**（不处理 `tenant_brand=lark` 的域名切换）。
- **不做 `tenant_access_token` 进程内缓存**。
- **不发手动启用/停用通知**。
- **不重试、不排队、不做发送失败补偿、不做通知流水表**（只存最近一次结果）。
- **不做交互卡片**（只发 `text`）。
- **不做多通道抽象**（只有飞书；不建通道类型注册表/插件机制）。
- **不实现 `addons` 编码**（默认应用模板已含所需权限与机器人能力）。
- **不做注册会话的多会话管理**（单会话槽位，新 `begin` 替换旧的）。
- **不做通知的历史查询界面**、不做按事件类型的订阅开关（要么全发要么不发）。

## Further Notes

- 触发契约的现成基础：`src/availability.rs` 的四个转换动作**已经**只在确有状态变化时返回 `Ok(true)`（幂等空转返回 `Ok(false)`）——「只发真实转换」直接复用该契约，不需要新判定。
- 生产部署后需 `PRAGMA table_info(notification_channel)` 验证迁移 27 生效（schema_migrations 版本守卫会静默吞掉撞号迁移；14/15 号段是旧方案残留，不可复用）。
- 扫码流程**无法自动化端到端测试**（需要真人用飞书客户端扫码），因此集成测试用 mock 覆盖状态机，真机验证留给人工。
- 需求来源与全部已拍板决策见 `.scratch/feishu-notification/REQUIREMENTS.md`；决策文档见 `docs/adr/0023-feishu-notification-self-hosted-http.md` 与 `docs/adr/0017-backup-export-and-restore.md`，术语见 `CONTEXT.md`「通知域」。
