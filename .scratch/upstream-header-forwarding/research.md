# RESEARCH — LLM/OpenAI 兼容中转站 HTTP 请求 Header 传递最佳实践

Status: research（供 `upstream-header-forwarding` 特性后续 spec/tickets 引用）
日期: 2026-09-03
范围: 下游（client→gateway）请求头如何过滤/透传/覆盖为上游（gateway→provider）请求头；本仓库当前为 `/v1/chat/completions` 单端点、四协议（OpenAI Compatible / OpenAI Responses / Anthropic / Gemini）、HTTP/1.1-only 上游。
结论先行见文末「10. 推荐最小实现」，全文每项主张后附一手来源链接。

---

## 0. 现状（仓库代码事实，不改动代码仅记录）

| 环节 | 位置 | 行为 |
| --- | --- | --- |
| 下游头读取 | `src/routes/openai_compat.rs:27-33` | `chat_completions` 的 handler 签名只提取 `State`/`Extension(AuthedApiKey)`/`Json(body)`，**未读取 `HeaderMap`，全部下游头在入口被丢弃**（包括 `User-Agent`、`x-request-id`、`traceparent`、`OpenAI-*`、`anthropic-*` 等）。 |
| 鉴权 | `src/auth/mod.rs:242-304` | `/v1/*` 经 `auth_middleware` 校验 `Authorization: Bearer <lg- 网关key>`；鉴权通过后 `AuthedApiKey` 注入 extension。会话 Cookie 仅用于 `/api/*`。 |
| 上游头组装 | `src/proxy/mod.rs:471-581` | `build_upstream_call` 按协议生成 `auth_headers`（OpenAI 系 `authorization: Bearer {成员key}`；Anthropic `x-api-key` + `anthropic-version: 2023-06-01`；Gemini `x-goog-api-key`），随后 `merge_custom_headers(&member.custom_header, &mut headers)` **把 provider `custom_header`（JSON 对象）Vec 追加**到 `auth_headers` 之后。 |
| 上游头固定值 | `src/proxy/upstream.rs:508-514` | `send_upstream_request` 无条件 `Builder` 设置 `Host=<authority>`、`Content-Type: application/json`、`accept: application/json, text/event-stream`、`Content-Length=<body.len()>`，然后遍历 `call.headers` **逐条 `.header()` 追加**。 |
| 潜在重复值 | 上述两处 | `HeaderValue` 追加语义：若 `call.headers` 里含 `authorization`/`content-type`/`accept`/`host`/`content-length`（如 custom_header 配了同名键），hyper `Builder` 会**产生重复的逗号合并/多行头**——`Content-Length`/`Host` 出现两份是 framing 违规，`Authorization` 两份语义含糊，下游可借此注入。 |

由此产生三个明确缺陷：① 下游头整批丢弃（无法把 trace 上下文/org 归属/请求 id 透传）；② `custom_header` 与协议固定头、与网关生成鉴权头可同名重复；③ `Content-Length`/`Host` 为固定生成值，一旦重复会破坏 HTTP 语义。

补充事实：`x-request-id` 网关已自行生成 UUID 并写入内部 `request_id` 字段（`src/proxy/mod.rs:764`、`metrics.rs`），但**不设到任何 HTTP 头**；`test_model` 与模型刷新（`src/provider_model/refresh.rs:67-75`，reqwest 直连）同样各自组装 custom_header，与 `/v1` 转发路径共享相同规则。

---

## 1. HTTP 头传递的总原则：end-to-end vs hop-by-hop

### 1.1 RFC 9110 的转发框架（本方案的所有规则之源）

- 代理/中转站（intermediary）**只要不是隧道（tunnel），必须实现 Connection 头语义**：转发前解析 `Connection` 头，把它列出的每个 connection-option 对应的同名头（含 trailer）删掉，然后删掉/替换 `Connection` 本身。这是 HTTP/1.1 区分「只给直接对端（hop-by-hop）」与「给链路上所有人（end-to-end）」的声明式机制。
  > RFC 9110 §7.6.1: “Intermediaries MUST parse a received Connection header field before a message is forwarded and, for each connection-option in this field, remove any header or trailer field(s) from the message with the same name as the connection-option, and then remove the Connection header field itself (or replace it with the intermediary's own control options for the forwarded message).”
  来源: https://www.rfc-editor.org/rfc/rfc9110.html#section-7.6.1
- 即使是标准头，只要其语义只针对当前连接，代理转发前也应移除，**无论它是否出现在 Connection 头里**。RFC 明示包括（不限于）：`Proxy-Connection`、`Keep-Alive`、`TE`、`Transfer-Encoding`、`Upgrade`。
  > RFC 9110 §7.6.1: “intermediaries SHOULD remove or replace fields that are known to require removal before forwarding… This includes but is not limited to: Proxy-Connection… Keep-Alive… TE… Transfer-Encoding… Upgrade”
  来源: https://www.rfc-editor.org/rfc/rfc9110.html#section-7.6.1

### 1.2 对 LLM 网关的推论

LLM 网关通常是一个**语义上的「内容网关/反向代理」**（转发 POST JSON、可能改写 body 做协议转换），连接不透明：client→gateway 与 gateway→provider 是两条独立 TCP/TLS 连接。因此：

- client 发给 gateway 的 hop-by-hop 头（`Connection`、`Keep-Alive`、`Proxy-Connection`、`Upgrade`、`TE`、`Transfer-Encoding`、以及任何被 `Connection` 指名剥离的头）**一律不得透传**给上游。
- 上游请求应视为一个**全新生成**的 HTTP/1.1 请求：`Host`/`:authority` 由网关基于上游 URL 生成（RFC 9112 §3.2.2：代理转发 absolute-form 时必须按 request-target 重新生成 Host，而不是透传收到的 Host）；framing（`Content-Length`）由网关基于**重写后**的 body 重新计算（请求体可能被协议转换改写）。

### 1.3 Host

- 当前行为 `src/proxy/upstream.rs:510-511` 固定 `.header(HOST, authority)`（由 `scheme://host:port` 计算，缺省端口省略）——**这是正确且应保持的**。理由：
  - RFC 9110 §7.2：Host 是**应用层路由机制**，也正是缓存投毒/错误路由的常见攻击点；代理必须按自己发出的请求的目标重新生成 Host，而不是透传客户端 Host。
  - RFC 9112 §3.2.2（absolute-form）：网关以 absolute-URI 转发时，MUST 忽略收到的 Host 并以 request-target 的 authority 重新生成 Host。
  来源: https://www.rfc-editor.org/rfc/rfc9110.html#section-7.2 、https://www.rfc-editor.org/rfc/rfc9112.html#section-3.2.2
- **结论：上游 Host 永远取 member 的 base_url authority，下游任何 `Host` 头都不透传、无例外。**

### 1.4 Content-Length / framing

- 当前行为 `src/proxy/upstream.rs:512-514` 固定 `Content-Length = body.len()`；且协议转换可能改 body（`build_request_body` 重写 JSON），所以**长度必须基于出站 body 计算**。
- RFC 9112 §6.3（body length 优先级）：同时收到 `Transfer-Encoding` 与 `Content-Length` 时以 TE 为准、CL 应被移除，且这类消息可能是 request smuggling；RFC 9112 §11.2 定义了 request smuggling 场景与防御（下游对 framing 不一致必须 400/502 关闭连接）。
  来源: https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3 、https://www.rfc-editor.org/rfc/rfc9112.html#section-11.2
- 结论：出站请求**永远只发一条由网关按出站 body 长度计算的 `Content-Length`**；下游的 `Content-Length`/`Transfer-Encoding`/`TE`/`Trailer` 一律不进上游头。axum 侧已把下游 body 完整读取为 `Json<Value>`（有 DefaultBodyLimit 5 MiB），故不会出现 chunked 转发问题。

### 1.5 Content-Type / Accept

- `Content-Type: application/json`：RFC 9110 §8.3 建议发送方生成带 content 的消息时带正确 media type。出站 body 恒为 JSON（四协议都是），固定 `application/json` 合理；若未来有二进制多模态（图片以 JSON base64 内嵌，仍是 JSON），也无需改。**不要透传下游 Content-Type**（可能缺 charset、可能是别的类型，转发语义混乱）。
- `Accept: application/json, text/event-stream`：网关同时请求流式与非流式两种返回，固定值合理且已覆盖 Anthropic/Gemini/OpenAI 的 SSE。**不要透传下游 Accept**（OpenAI 客户端常发 `Accept: application/json`，会卡掉 SSE；见 §7 流式）。两者都应归类为「网关生成、防重名覆盖」的头。

---

## 2. 必须剥离的 header 全清单

分四类，全部不进上游。这是本方案可测试的安全基线。

**A. hop-by-hop / connection 管理（RFC 9110 §7.6.1 / RFC 9112）**
`Connection`、`Proxy-Connection`、`Keep-Alive`、`Upgrade`、`TE`、`Transfer-Encoding`、`Trailer`、`Proxy-Authenticate`、`Proxy-Authorization`。
（`Proxy-Authenticate`/`Proxy-Authorization` 是 RFC 9110 保留的、语义限定于代理跳的头，见 §11.7/§11.7.1。）

**B. framing / 表示元数据（由网关重新生成）**
`Host`、`Content-Length`、`Content-Type`、`Accept`、`Content-Encoding`、`Content-Language`、`Content-MD5` 等表示层元数据不应从下游搬移到出站请求（出站 body 是网关重写的 JSON）。`Expect: 100-continue` 也必须在入口消费掉/不转发（axum 已缓冲 body）。

**C. 凭据与身份隔离（见 §3）**
`Authorization`、`Cookie`、`x-api-key`、`x-goog-api-key`、`x-amz-*`（签名用）、`Ocp-Apim-Subscription-Key`、`API-Key`、自定义 `X-Api-*` 等全部按 §3 处理。

**D. 入站路由 / 会污染上游的链路头**
`X-Forwarded-For`、`X-Forwarded-Proto`、`X-Forwarded-Host`、`Forwarded`、`Via`、`X-Real-IP`：除非显式开启「追加式」透传（RFC 7239 §7.2 仅在直接转发时「preserve and possibly extend」），LLM 网关面对的是「一个入站请求 → 一次出站（或一次 failover 多次出站）」，且上游是**模型 API 不是 next-hop proxy**，这些头对 provider 无意义且是 client 可伪造的（RFC 9110 §7.6.1 关于伪造 host/xff 的投毒警告同样适用）。
来源: https://www.rfc-editor.org/rfc/rfc7239.html#section-7.2 、https://www.rfc-editor.org/rfc/rfc7239.html#section-8.3

---

## 3. 下游 Authorization / Cookie / API key 如何隔离

### 3.1 原则：三层凭据绝不混池

一个请求在链条上存在三个不同主体的凭据：
1. **下游调用网关**的凭据（本仓库：`lg-` key 放在 `Authorization: Bearer`）；
2. **Provider custom_header** 中管理员为某供应商配的凭据/额外头（管理员信任域）；
3. **gateway→provider** 由网关生成的协议鉴权头（成员 api_key 解密后即时生成）。

任何一层都**不得**以「透传下游同名头」的方式与另一层合并；下游凭据不得到达上游。

### 3.2 规范依据

- RFC 9110 §11.6.2：代理转发请求时 **MUST NOT 修改** Authorization；同时它是**端到端用户代理→源服务器**语义的凭据（§3.5 [CACHING] 说明缓存不能把带 Authorization 的响应发给其他用户）。LLM 网关的下游 key 是「用户代理→网关」的凭据，出站请求的源服务器是 provider——把下游 Authorization 直接搬到上游，等于让 provider 拿到网关用户凭据，属凭据越界（by-credential 泄漏）。
  来源: https://www.rfc-editor.org/rfc/rfc9110.html#section-11.6.2
- **行业实现先例（LiteLLM）**：`clean_headers()` 会先剥离 `SpecialHeaders`（枚举即：`Authorization`、`API-Key`、`x-api-key`、`x-goog-api-key`、`Ocp-Apim-Subscription-Key`、`x-litellm-api-key`），转发器 `_get_forwardable_headers()` **只放行 `x-*` 前缀（除 `x-stainless-*`）与 `anthropic-beta`**；网关鉴权头（Authorization/x-api-key）与 provider 鉴权头由两条独立路径写入出站请求，绝不靠透传。即使开启 BYOK（`forward_llm_provider_auth_headers: true`），也是把客户端 `x-api-key` **显式作为 provider 凭据参数**注入，而不是把下游 Authorization 透传。官方文档明确列出 allowlist 表：`Authorization`/`Content-Type`/`Host`/`Accept`/`User-Agent` 均**不透传**。
  来源: https://docs.litellm.ai/docs/proxy/forward_client_headers 、源码 https://github.com/BerriAI/litellm/blob/main/litellm/proxy/litellm_pre_call_utils.py（`clean_headers` L903、`_get_forwardable_headers` L1052）与 SpecialHeaders 定义 https://github.com/BerriAI/litellm/blob/main/litellm/proxy/_types.py#L4239
- LiteLLM 还因此翻过车（issue #32202：#19618 指出 pass-through + `forward_headers:true` 曾把代理 Authorization 上游泄漏）。这印证「allowlist 放行 + 鉴权头单独注入」而不是「denylist 过滤」才是稳妥设计。

### 3.3 本仓库落地建议

- 入口中间件把 `Authorization`（Bearer lg- key）读出用于 `api_key` 表校验后，**该头即从透传候选移除**；`AuthedApiKey` 携带 key 名（`name`）注入 extension，用于落库与日志，不向外发。
- 上游出站只写两处凭据：
  - 协议鉴权头（gateway 生成，解密 provider key）；
  - 若开启 BYOK 式能力（可选配置），把「客户端在**受信任头**里提交的 provider key」**显式转写**为该协议要求的鉴权头，且优先级低于 gateway 配置的 key（或要求 gateway key 为空才允许）。默认关闭。
- `Cookie` 头：RFC 9110 §9.8.6 明确 Cookie 的作用域是 host（“apply to all origins with the same host”），下游会话 Cookie 对 provider 无意义且是会话固定/劫持面；**绝不透传**，也不应作为 custom_header 默认放行项。

---

## 4. 三类 header 的优先级

需要建模为三层，每层内部不可变，层间用**固定规则**而不是时间顺序去重：

| 优先级 | 层 | 说明 |
| --- | --- | --- |
| **1（最高，网关固定）** | framing/协议框架头 | `Host`、`Content-Length`、`Content-Type`、`Accept`(对 stream 是网关侧决定)。永远最后写入且**强制覆盖**同名。 |
| **2（网关生成，协议必需）** | 协议鉴权头 + 语义必需头 | OpenAI 系 `Authorization: Bearer`；Anthropic `x-api-key`、`anthropic-version: 2023-06-01`；Gemini `x-goog-api-key`。成员解密 key 即时拼装。 |
| **3（管理员配置，供应商域）** | provider `custom_header` | 管理员声明式“这个供应商要额外带什么”。允许覆盖/补充除第 1 层外的头；对第 2 层头的同名冲突应**策略可选**（默认：custom_header 不得覆盖协议鉴权头，冲突时以第 2 层为准 + warn；若管理员明确要覆盖鉴权，可给 per-provider 开关，见 §10 风险）。 |
| **4（下游透传，默认最小集）** | allowlist 放行的下游头 | 见 §1 与 §7。只允许「语义中立/可提升观测」的头。**永远不能**覆盖第 1、2、3 层（实现上：下游透传最先写入，后写的固定/鉴权/custom 覆盖它）。 |

**why 第 3 层默认不能覆盖第 2 层**：否则管理员误配一个同名 `authorization`/`x-api-key` 的 custom_header，会让整条链路的凭据变成管理员手填值（易错、可能是旧 key、还可能明文泄露），而系统里明明配置了加密存储的 provider key。覆盖第 2 层应是一等公民开关 + 校验（warn），而不是“谁 append 在后谁赢”的静默行为。

当前代码正是**没有层模型**：`mod.rs:471-581` 先写第 2 层，再 append 第 3 层，第 1 层由 `upstream.rs` 最后写——所以 custom_header 里的 `Host`/`Content-Length` 会与第 1 层撞成重复值，`authorization` 会与第 2 层撞成重复值。必须改成「第 4 层 → 第 3 层 → 第 2 层 → 第 1 层」顺序且每层用 `insert`（覆盖）+ 显式剥离清单。

---

## 5. 重复/多值 Header：Set-Cookie 例外与请求侧规则

### 5.1 规范

- RFC 9110 §5.2 字段合并规则：除少数「特殊字段」外，同名多行可被语义合并为逗号列表；**Set-Cookie 是明确的例外**——它不能用逗号合并，必须逐行保留（每行是一个完整实例）。RFC 9110 §6.5 对 trailer 的合并规则同理有专门说明。
  来源: https://www.rfc-editor.org/rfc/rfc9110.html#section-5.2 、#section-6.5
- RFC 9110 §5.5（Field Values）：头名大小写不敏感、可重复；多值语义由具体字段定义。
  来源: https://www.rfc-editor.org/rfc/rfc9110.html#section-5.5

### 5.2 请求侧结论（本网关只处理请求，不处理响应 Set-Cookie）

- 出站请求**不应出现任何同名重复**（这是工程纪律，不是协议强制）：`Host`/`Content-Length`/`Content-Type`/`Authorization` 等重复值会触发 RFC 9112 §6.3 的 framing 错误（CL 重复且不等 → 400/502）或鉴权歧义。
- 多数请求头是单值语义（`User-Agent`、`x-request-id`、`traceparent`），只取**下游收到的第一条**（HTTP 客户端/SDK 不会发重复，但恶意 client 可能；网关统一 first-wins 即可，HTTP 语义上重复等同逗号列表的只有 `Forwarded`/`X-Forwarded-*`/`Via` 这类 list 字段——而这些我们默认不透传）。
- 若未来需要透传 list 语义头（如给某些 OpenAI 兼容 provider 的 `anthropic-beta` 多值），按逗号 `, ` join 为单行再出站（Anthropic 官方文档明确多 beta 用逗号分隔，见 §7）。
- **响应侧例外（提醒，不在本特性范围）**：`dispatch_success` 若未来要回传 provider 响应头，`Set-Cookie`、`X-RateLimit-*` 等需按响应特殊字段规则逐条透传、不能逗号合并；provider 不会设 Set-Cookie，故默认全部丢弃即可。
- 实现建议：hyper/`HeaderMap` 的 `insert`（覆盖）与 `append`（多值）二选一，且**出站构建禁止对已存在 key 调 append**（提供单测断言：对剥离清单内 key，出站后 `.get_all().len()==1`）。

---

## 6. traceparent / tracestate / baggage / x-request-id

### 6.1 traceparent / tracestate（W3C Trace Context）

- **traceparent**：固定 `version-traceid-parentid-flags`；接收方必须「原样发给所有出站请求」（“A vendor receiving a traceparent request header MUST send it to outgoing requests”）；纯转发（pass-through）服务**不改值**，改值的唯一合法动作（改 sampled/restart）都要求同时更新 parent-id，而代理若既不改也不参与 trace，则**不得修改**；若没改 traceparent，tracestate 也 MUST NOT 改动。
  来源: https://www.w3.org/TR/trace-context/ §3.4 Mutating the traceparent Field、§3.5、§4 Processing Model
- **tracestate**：厂商私有 KV 列表，最多 32 项；转发时**必须透传**（“Every tracing tool MUST properly set traceparent even when it only relies on vendor-specific information in tracestate”）。若 gateway 自身是 trace 端点（参与 trace），它应把自身条目加在**最左**并把上一跳同名 key 右移（重写其自己的条目）。
  来源: https://www.w3.org/TR/trace-context/ §3.3
- 校验：无效 traceparent（非 hex、全零 trace-id/parent-id、错误长度）MUST ignore；转发前网关可做轻量格式校验（至少长度+hex）以抵御注入。长度建议：tracestate 应传播至少 512 字符（§3.3.1.5），出站前如超限按整条条目截断。
  来源: https://www.w3.org/TR/trace-context/ §3.2.2.3/.4、§3.3.1.5

### 6.2 baggage（W3C Baggage）

- 语义中立 KV，**应**被传播（“Libraries and platforms SHOULD propagate this header”），可被中间层改写/过滤；上限 8192 bytes / 64 个 member，超限可整条丢弃；**跨信任边界应过滤敏感项**（baggage 可能携带 PII）。
  来源: https://www.w3.org/TR/baggage/ §3.3.2 Limits、§4 Security/§5 Privacy
- 结论：作为可选透传项。默认**不开启 baggage 透传**（内容完全由下游 client 控制，可携带任意 PII 与任意大 payload → 上游 + 日志面）；若开启需 8 KiB 截断 + 拒绝超限请求（400）或丢弃该头。

### 6.3 x-request-id

- **对 OpenAI 而言 `x-request-id` 是响应头，不是请求头**（“Inspect HTTP response headers for the unique ID of a request”）；客户端请求里带的自定义关联 id 通常叫 `X-*`/`Idempotency-Key`。OpenAI 文档另外给出硬约束：请求总头 ≤64 KiB、自定义头合计 ≤60 KiB，超出会在请求到达 API 前被拒（可能无响应也无 x-request-id）。
  来源: https://platform.openai.com/docs/api-reference/authentication（Request headers / Debugging requests 段）
- 本仓库已为每次转发生成 `request_id`（UUID v4）并用于落库/日志/SSE，与 OpenAI/Anthropic 响应里的 `request_id`（如 `req_…`）是两码事。建议：
  - 可选把入站 `x-request-id` 视作「外部调用方 id」，接受并**回写响应头**（若下游 header 透传默认不透 `x-request-id`，则它作为响应头返回可让调用方对齐日志）。注意不要覆盖 provider 响应自身的 `x-request-id`。
  - **透传决策**：`x-request-id` 语义中立，可作为 allowlist 透传项（LiteLLM 就把 `x-request-id`/`x-trace-id` 列入透传示例）；但若网关把 `request_id` 也写进出站 `x-request-id` 则会与透传的下游值冲突——建议：**要么**透传下游原值、**要么**网关覆盖为自身 UUID（二选一，配置化），不要在透传后又覆盖。

### 6.4 本仓库落地建议

- 入站 `traceparent`/`tracestate` 原样透传出站（仅做基本 hex/长度校验；不合法则忽略）。可选再做：网关以 `traceparent` 派生自己的 span id（更新 parent-id + 左插 tracestate 条目）——但那要求实现 trace exporter，超出本特性范围，先做透传。
- `x-request-id` 走配置：`forward_client_request_id` 默认 false（即默认不透，保持现状、避免与 provider 响应的 id 混淆）；开启时透传下游值且网关不再覆盖。
- baggage 默认不透传（可选开关 + 截断）。
- 所有透传项的写入都**早于**第 2/1 层（允许被 custom_header 覆盖，不允许覆盖鉴权）。

---

## 7. 语义 header：User-Agent / OpenAI-* / Anthropic-* / Idempotency-Key 等

| Header | 语义 | 是否透传 | 依据 |
| --- | --- | --- | --- |
| `User-Agent` | 客户端标识；OpenAI 把 UA 与总头预算一起计入（64 KiB）。OpenAI/Anthropic 官方 SDK 都会发 UA。 | **否（网关生成自己的 UA，如 `llm-gateway/0.1.9`）** | OpenAI: “keep the total size of an API request’s headers under 64 KiB… including… User-Agent”。网关身份应由网关声明，而不是替下游冒认。LiteLLM 亦不透传 UA。provider 风控/计量可能读 UA，冒充下游 UA 无意义且有害。 |
| `OpenAI-Organization` / `OpenAI-Project` | 指定 OpenAI org/project 归属，**影响计费与权限**（“Usage from these API requests counts as usage for the specified organization and project”）。 | **按 provider 配置而非下游透传**：若某 provider 是 OpenAI 官方，管理员在 custom_header 配它才合理；下游任意传会越权指定别人 org（若 org id 有效则计费错乱，无效则请求失败）。**默认不把下游的 `OpenAI-Organization/Project` 透传给 OpenAI 官方 provider**（会让网关内部虚拟模型混用不同客户 org → 计费归属不可控）。可选支持：provider 上配 `forward_openai_org_id=true` 时透传（LiteLLM 有此精确开关 `get_openai_org_id_from_headers`）。 | https://platform.openai.com/docs/api-reference/authentication ；LiteLLM 源码 https://github.com/BerriAI/litellm/blob/main/litellm/proxy/litellm_pre_call_utils.py#L1125 |
| `anthropic-version` | **Anthropic 必须**（“you must send an anthropic-version request header”）。值 `2023-06-01` 或更新。 | 网关生成（当前固定 2023-06-01）；下游是 OpenAI 兼容入口，天然不会带它，即使带也应忽略——版本策略由 provider 配置管。 | https://docs.anthropic.com/en/api/versioning |
| `anthropic-beta` | 可选项；启用 beta 功能。多 beta 用逗号分隔（`feature1,feature2`）；Anthropic 官方**会拒绝未知 beta 名**（“Unexpected value(s) invalid-beta-name…” → 400）。 | **LiteLLM 把它列为仅有的两个透传白名单之一**；但本仓库下游是 OpenAI 兼容入口，anthropic-beta 不会出现于下游，透传面来自：① provider custom_header 配；② 若未来暴露 Anthropic 原生端点。因此把 `anthropic-beta` 作为「下游可透传集合」之一实现成本低，但当前实际流量不会触发。注意：透传时**逗号 join 单值**，且未知 beta 会 400（风险由 provider 配置承担）。 | https://docs.anthropic.com/en/api/beta-headers |
| `x-api-key` | Anthropic/Gemini 等上游鉴权名。 | 见 §3：非透传，作为第 2/3 层凭据。 | https://docs.anthropic.com/en/api/versioning |
| `x-goog-api-key` | Gemini 鉴权（“The API key… sent as an x-goog-api-key header”）。 | 网关生成（第 2 层）。 | Gemini 文档（ai.google.dev/gemini-api/docs/api-key，经 search/extract 间接核对：API key 经 `x-goog-api-key` 头发送）。 |
| `anthropic-organization-id` | 与 OpenAI-Organization 同类（release notes 2025-02-10 引入），**可选**，用于跨 workspace 计费归属。 | 同 OpenAI-Organization：provider 级配置，不透传下游。 | https://docs.anthropic.com/en/release-notes/api |
| `Idempotency-Key` | Stripe 语义：client 生成、服务端按 key 存首响应用于重试幂等；POST 才接受；最多 255 字符；key 含敏感信息会被拒。OpenAI 兼容/chat 端点**不接受**（OpenAI 未把它列入请求头；实际不识别会忽略）。 | **不适用**：chat/completions 生成式端点天然非幂等，provider 端不会实现幂等；透传无益且下游若连到不支持它的中间层可能报错。若网关自己做「相同 body + 相同 key → 去重」那是网关缓存层，本特性不做。 | https://docs.stripe.com/api/idempotent_requests |

### 通用规则小结
- 协议**必须**头（`anthropic-version`、`authorization`、`x-api-key`、`x-goog-api-key`）：第 2 层生成，绝不被下游/custom_header 覆盖。
- **归属/计费**头（`OpenAI-Organization/Project`、`anthropic-organization-id`）：敏感 → provider 级显式配置才发，默认不透传下游。
- **能力开关**头（`anthropic-beta`）：下游可透传 allowlist 项（当前流量为空），逗号 join、可被 custom_header 覆盖。
- `User-Agent`/`Accept`/`Accept-Encoding` 等协商/身份头：网关自己生成，不透传下游。

---

## 8. 流式请求有没有特殊 header

- OpenAI 兼容流式 = 响应 `Content-Type: text/event-stream` 的 SSE。**响应**侧特殊性大（本仓库 `sse_response` 已设 `content-type: text/event-stream`、`cache-control: no-cache`、`connection: keep-alive`，见 `src/proxy/mod.rs:1594-1606`——注意这里手工写了 `connection: keep-alive`，在 axum/hyper HTTP/1.1 下属于正确且常用，但若未来走 HTTP/2 需要按协议处理，见下文兼容性）。**请求**侧对流式没有专门头：
  - 网关用 `Accept: application/json, text/event-stream` 表达「两种都行」；不要透传下游 `Accept: application/json`，否则会请求到非流式响应，与 `stream=true` 语义冲突。
  - Anthropic 流式 = `POST /v1/messages?stream=true` 或 body `stream: true`；Gemini 流式 = 请求带 `alt=sse` 或（SDK 层）streamGenerateContent；本仓库统一走「body stream 标记 + 转换器逐事件重写」而非原样 SSE，请求头与流式无关。
  - 上游在流式时不该收到 `Content-Length` 语义改变——出站仍是完整 JSON（一次发完，长度已知），保持 `Content-Length`。**代理真正的流式点在于响应体不设总超时**（`upstream.rs` 对 `stream=true` 只等 header、体逐帧转发），这不影响请求头。
- SSE 特殊点（若未来要原样透传 provider 响应头）：
  - `Content-Type` 必须由网关写 `text/event-stream`（当前正确）；provider 可能发 `text/event-stream; charset=utf-8`，透传拼接要小心不要重复。
  - 响应 `Connection` 头对 HTTP/1.1 有 keep-alive 语义——当前手工写 `keep-alive` 是对的，但它是**跳级头**：HTTP/1.1 每个中间跳都重新协商。留作兼容性备注。

---

## 9. SSRF / header injection / 日志脱敏

### 9.1 header injection / request smuggling 防御

- 转发前必须把「不能进上游」的头彻底剥掉（§2），否则：恶意 client 放 `Content-Length: 0` + 合法长度 → 若透传产生重复 CL，触发 RFC 9112 §11.2 走私面；放伪造 `Host` 企图劫持虚拟主机路由（RFC 9110 §7.2 明示 Host 是投毒/错误路由高频攻击点）；放 `Connection: keep-alive, x-custom` 之类指名要剥的头。
- hyper `HeaderValue::from_str` 已拒绝 CR/LF，header 值**不可能**注入换行；但仍需（1）在**入口**拒绝/截断超长头（axum/hyper 有默认限制；OpenAI 建议总头 ≤64 KiB、自定义 ≤60 KiB——可作为网关对下游的校验值，超限 431/400）；（2）header **名**用 `HeaderName::from_bytes` 校验（现代码已做）。
- custom_header 是从管理后台来的 JSON——**把它当不可信输入**：值仅允许合法 `HeaderValue`，非字符串值跳过（现代码已做）；拒绝 admin 把 `Content-Length`/`Host` 写进 custom_header（第 1 层强制覆盖，或直接 400 提示）。

### 9.2 SSRF

- provider `base_url`/proxy 是管理员配置，网关不把下游任何「URL/主机」类头（`X-Forwarded-*`、`Forwarded`、`Host`）用于寻址——出站目标只来自数据库 member 配置 + 第 1 层生成的 authority，天然无 SSRF 透传面。要防的仅是 provider `base_url` 本身被配成内网（那是管理面问题，与 header 无关，可提示不强制）。

### 9.3 日志脱敏

- 现状：`src/proxy/mod.rs` 各失败路径 `fail_reason` 落库（如 `truncate_chars(message, 200)`），message 主要来自上游错误体，**不含 header**；`upstream.rs` 错误仅域名/端口（`{host}`），安全。但 `tracing::debug!(request_id, ...)` 决策日志不含头。
- 一旦引入「透传下游头」，必须遵守：
  1. **落库/日志永不记录 Authorization/Cookie/x-api-key/x-goog-api-key 明文**。LiteLLM 专门实现 `redact_credential_headers()`（把 `_CREDENTIAL_HEADER_NAMES` = SpecialHeaders ∪ {cookie, proxy-authorization} 打码为 `***REDACTED***`）——同类函数在网关内必须存在，且要覆盖「透传的原值」与「第 2 层生成的 key 明文」两个来源。
    来源: https://github.com/BerriAI/litellm/blob/main/litellm/proxy/litellm_pre_call_utils.py#L961（`redact_credential_headers`）
  2. 若新增「请求头审计/透传头回显」日志，只记**名称集合**（`forwarded_headers: [x-trace-id]`）或脱敏值，不要记值。
  3. 上游错误信息 `extract_error_message` 是 provider 返回的错误 JSON（OpenAI 错误体不会回显你的请求头；但个别 OpenAI 兼容上游可能把收到的头 echo 进错误——这是 provider 侧问题，无法在网关全防，只能日志侧兜底脱敏）。

---

## 10. 推荐最小实现 + 测试矩阵 + 兼容性风险 + 可选配置

### 10.1 推荐最小实现（顺序即管线）

1. **入口收集**（`routes/openai_compat.rs`）：handler 增加 `headers: HeaderMap` 参数（`Json` 提取器可与 `HeaderMap` 共存），或改用 `Request` 提取。至少保留一次使用的：`Authorization`（已被中间件用掉）、以及需透传的 allowlist 项。**不改变鉴权流程**。
2. **构造上游头**（`proxy/mod.rs::build_upstream_call` 重构为三层模型）：
   - 输入：`member`（含 custom_header）、协议、解密 key、**下游头（allowlist 过滤后的子集）**、内部 `request_id`、body bytes。
   - 输出：**唯一的最终 HeaderName→HeaderValue 有序表**，而不是两个 Vec。顺序：下游 allowlist 透传 → custom_header（`insert` 覆盖透传层）→ 协议鉴权/必需头（`insert` 覆盖 custom_header，除非该 provider 开启 allow-override-auth 开关）→ 框架头（`Host`/`Content-Type`/`Accept`/`Content-Length`，`insert` 强制覆盖）。
   - 剥离清单常量（§2 的 A/B/C/D + `connection` 命名的动态剥离：解析入站 `Connection` 头，其 value 各 token 对应的头一并删）。
   - `upstream.rs::send_upstream_request` **删除** `.header(HOST)`/`Content-Type`/`accept`/`Content-Length` 之后的自由追加——改为：框架头最后无条件覆盖一次（或断言 `call.headers` 不包含框架头）。同时避免与 `Connection: keep-alive` 语义冲突（当前 `sse_response` 手工写该头保持现状）。
3. **allowlist 默认集**（建议，可配置）：
   - 透传：`traceparent`、`tracestate`、`anthropic-beta`（逗号 join）、`x-request-id`（可选开关）。
   - 可选透传：`baggage`（8 KiB 截断）、任意 `x-*`（除 `x-stainless-*`、`x-goog-api-key`、`x-api-key`、`x-amz-*`——**凭据名一律走剥离清单**）。
   - 永不透传：§2 全清单 + `OpenAI-*`/`anthropic-organization-id`（除非 provider 级开关）+ `Idempotency-Key`。
4. **协议鉴权头组装逻辑保留**在 `build_upstream_call`，仅改为「覆盖式」写入最终表。
5. 向出站注入**网关自己的标识头**（可选但推荐）：`User-Agent: llm-gateway/<version>`。
6. 若不透传 `x-request-id`：可在成功响应用 `X-Request-Id`/`x-request-id` 回传给下游（OpenAI 生态客户端常读它）；透传上游 provider 响应头的功能不在本特性范围。

### 10.2 测试矩阵（新增集成测试到 `tests/proxy_integration.rs` / 单测到 `proxy/mod.rs` 内）

| # | 场景 | 断言（mock 上游抓到请求头） |
| --- | --- | --- |
| 1 | 下游带 `traceparent`/`tracestate` | 上游收到**原样** traceparent/tracestate 单值 |
| 2 | 下游带 `Authorization: Bearer lg-xxx` | 上游的 `Authorization` = `Bearer <provider 解密 key>`，**不含** lg-xxx |
| 3 | 下游带 `Cookie`/`Proxy-Authorization` | 上游**无** Cookie/Proxy-Authorization |
| 4 | 下游带 `Host: evil.example` | 上游 `Host` = member base_url authority |
| 5 | 下游带 `Content-Length: 1` / `Transfer-Encoding: chunked` / `Connection: keep-alive, x-foo` | 上游 CL = 真实出站 body 长度；无 TE/Connection/x-foo |
| 6 | custom_header 配 `{"Authorization":"Bearer x","anthropic-version":"2023-06-01","X-A":"b"}` | Anthropic 成员上游：`authorization`→实际用 provider key 的值（默认 custom 不覆盖）；`anthropic-version`→2023-06-01；`X-A`→b。**无重复行** |
| 7 | 下游带 `x-request-id: client-1`（开启 forward） | 上游 `x-request-id: client-1` |
| 8 | 流式 & 非流式各跑一遍 | `Accept` 恒 `application/json, text/event-stream`；`Content-Type: application/json` |
| 9 | failover 两次出站 | 每次出站头一致（同一请求同一下游头快照），不串 |
| 10 | 下游带超长/非法头名或 `x-stainless-lang` | 被剥离或 400，不进上游 |
| 11 | custom_header 覆盖协议的**开关打开**时 | 允许覆盖，记 warn |
| 12 | 四协议各一次（OpenAI Compat/Responses/Anthropic/Gemini） | 各协议鉴权头名称正确、只有一份 |
| 13 | `x-request-id`（上游响应）与网关 request_id 分离 | 不透传时上游无入站 x-request-id（不把内部 UUID 泄给 provider 当 request id） |
| 14 | 下游无任何透传头 | 出站头 == 框架 + 鉴权 + custom（最小集回归） |

### 10.3 兼容性风险

- **风险1（HTTP/2）**：上游 `upstream.rs` 只做 HTTP/1.1。HTTP/1.1 要求保留 `Host`（RFC 9112）；若未来接 HTTP/2 provider，框架头要改为 `:authority` 伪头、去掉 `Connection`、`keep-alive` 语义变化、`Transfer-Encoding` 禁用。现方案留好「框架头单独一层、一处生成」便于迁移。
- **风险2（下游经 HTTP/2 来）**：axum 已处理 h2→h1 转换并把 `:authority` 写入 Host 可见性，本方案不依赖下游连接版本。
- **风险3（stream 响应头）**：`sse_response` 手工 `connection: keep-alive`——hyper 在 HTTP/1.1 下支持；若网关启用 HTTP/2 出站，`connection` 头应删除（RFC 9113）。当前不入站变化。
- **风险4（行为变化对既有客户）**：现在下游头全丢，若客户依赖某种「我的 X- 头被透传」尚不存在；新功能默认 keep 现状（全丢 + allowlist 0 项），通过配置逐步放开，规避破坏性。
- **风险5（OpenAI SDK 自带头）**：OpenAI/Anthropic 官方 SDK 会带一堆内部头（`x-stainless-*`、`anthropic-version`、UA）。仅当未来暴露「原生 Anthropic/OpenAI 端点」时才需要处理「SDK 头 vs 网关头」冲突；对 OpenAI 兼容入口，下游多为简单客户端，风险低。参考 LiteLLM：`x-stainless-*` 明确不透传（会导致上游 SDK 行为异常）。
- **风险6（头大小）**：透传 allowlist 后总头可能变大；对入站建议加「总头 ≤ 64 KiB / 自定义 ≤ 60 KiB」的软上限校验（OpenAI 对出站 API 也这么限），超限 431 Request Header Fields Too Large。
- **风险7（trace 隐私）**：traceparent/tracestate 原样透传 = 把下游 trace 树暴露给 provider；某些客户不愿。可选 `forward_traceparent` 开关（默认 true 仍安全，因为 trace 头内容非凭据，不含 Cookie/token 类敏感值）。

### 10.4 可选配置（建议全部默认关闭/保守）

```
forward_client_headers_allowlist: []            # 例: ["x-trace-id","x-session-id","anthropic-beta"]
forward_traceparent: true                       # traceparent/tracestate 原样透传
forward_x_request_id: false                     # 透传下游 x-request-id（否则不回填也不透传）
forward_baggage: false                          # baggage 透传（8KiB 截断）
forward_openai_org_project: false               # 透传 OpenAI-Organization/OpenAI-Project
allow_custom_header_auth_override: false        # 允许 provider custom_header 覆盖协议鉴权头（warn）
upstream_user_agent: "llm-gateway/{version}"    # 网关 UA；空 = 不发 UA
```
归属：这些若做成设置项，落在 `src/app_settings` 的热更新机制里；若先不做设置项，则用常量 + 单测锁定，避免 YAGNI。

### 10.5 结论（回到 10 问的一句话答案）

1. 默认透传：`traceparent`/`tracestate`（可选 allowlist 加 `x-request-id` 与任意 `X-*`）。
2. 必须剥离：hop-by-hop（Connection/Keep-Alive/Proxy-Connection/Upgrade/TE/Transfer-Encoding/Trailer）、framing（Host/Content-Length/Content-Type/Accept/Content-Encoding）、凭据与身份（Authorization/Cookie/x-api-key/x-goog-api-key/Proxy-Authorization/API-Key 系）、路由头（X-Forwarded-*/Forwarded/Via/X-Real-IP）。
3. 隔离：下游 lg- key 只在入口鉴权，出站只写「解密 provider key 的协议鉴权头」；custom_header 里的鉴权名默认也不得覆盖协议层；BYOK 能力（若做）显式转写 + 默认关。
4. 优先级：框架头(1) > 协议鉴权/必需头(2) > provider custom_header(3) > 下游 allowlist 透传(4)；实现 = 唯一 HeaderMap 分层 insert。
5. 重复/多值：出站请求零同名重复（insert 语义）；list 型语义头逗号 join；Set-Cookie 是响应侧例外（本特性范围外）。
6. traceparent/tracestate 原样透传（不参与 trace 就不改值）；baggage 默认不透（截断可选）；x-request-id 配置化透传/回写，不与内部 request_id 混用。
7. UA 由网关自报；OpenAI-Organization/Project 与 anthropic-organization-id 属计费归属，provider 级显式开关才发；anthropic-version 网关生成；anthropic-beta 可作 allowlist 项（逗号 join）；Idempotency-Key 不透传。
8. 流式请求无专属请求头；关键是 Accept 固定双 media type、不把下游 Accept 透传、Content-Length 保持出站 body 长度。
9. 剥离即防走私/注入；出站目标只来自 DB 配置（无 SSRF 透传面）；日志/落库必须对凭据头做脱敏（引 LiteLLM `redact_credential_headers` 先例）。
10. 见上。

---

## 引用来源清单

### 标准/规范（一手）
- RFC 9110 (HTTP Semantics)：https://www.rfc-editor.org/rfc/rfc9110.html
  - §5.2/5.5 字段合并与多值；§6.5 Trailer；§7.2 Host；§7.6.1 Connection（end-to-end vs hop-by-hop、转发前剥离清单）；§7.7 消息变换；§8.3 Content-Type；§8.6 Content-Length；§9.8.6 Cookie；§11.6.2 Authorization；§11.7/11.7.1 Proxy-Authenticate/Proxy-Authorization
- RFC 9112 (HTTP/1.1)：https://www.rfc-editor.org/rfc/rfc9112.html
  - §3.2.2 absolute-form（proxy MUST 重生成 Host）；§6.1 Transfer-Encoding；§6.2/6.3 Content-Length 与 body length、CL+TE 冲突 = smuggling；§11.1/11.2 request/response splitting 与 request smuggling
- RFC 7239 (Forwarded)：https://www.rfc-editor.org/rfc/rfc7239.html
  - §5 参数；§6.3 obfuscated identifier；§7.2 Header Field Preservation；§8.3 Privacy
- W3C Trace Context：https://www.w3.org/TR/trace-context/
  - §3.2 traceparent（格式/校验/长度）；§3.3 tracestate（32 项/≥512 字符/截断整条）；§3.4 Mutating traceparent（原样透传/只允许两种改法/未改 traceparent 则 MUST NOT 改 tracestate）；§3.5；§7 Security（信息暴露、注入面）
- W3C Baggage：https://www.w3.org/TR/baggage/
  - §3.3.2 Limits（64 项/8192 bytes）；§3.5 Mutating；§4.1 Information Exposure（跨信任边界过滤）；§5 Privacy

### 厂商 API 文档（一手）
- OpenAI Authentication / Request headers：https://platform.openai.com/docs/api-reference/authentication
  - `Authorization: Bearer`；`OpenAI-Organization`/`OpenAI-Project` 用于计费归属；总头 ≤64 KiB、自定义 ≤60 KiB；`x-request-id` 是响应头；限流响应头清单
- Anthropic Versioning（`anthropic-version` 必填）：https://docs.anthropic.com/en/api/versioning
- Anthropic Beta headers（`anthropic-beta` 逗号分隔、未知 beta 名 400）：https://docs.anthropic.com/en/api/beta-headers
- Anthropic Messages API（x-api-key、headers 段）：https://docs.anthropic.com/en/api/messages
- Anthropic Release notes（`anthropic-organization-id` 2025-02-10）：https://docs.anthropic.com/en/release-notes/api
- Gemini API key 鉴权头（`x-goog-api-key`）：https://ai.google.dev/gemini-api/docs/api-key
- Stripe Idempotency-Key 语义（幂等层/255 字符/仅 POST/不用敏感数据）：https://docs.stripe.com/api/idempotent_requests

### 网关/中间件文档与源码（补充）
- LiteLLM 官方文档 Forward Client Headers to LLM API（allowlist 表：x-*/anthropic-beta 透传、Authorization/Content-Type/Host/Accept/User-Agent 不透传、BYOK 说明）：https://docs.litellm.ai/docs/proxy/forward_client_headers
- LiteLLM 源码（`clean_headers` L903、`_get_forwardable_headers` L1052、`redact_credential_headers` L961）：https://github.com/BerriAI/litellm/blob/main/litellm/proxy/litellm_pre_call_utils.py
- LiteLLM 源码 SpecialHeaders 枚举（Authorization/x-api-key/x-goog-api-key/Ocp-Apim-Subscription-Key 等）：https://github.com/BerriAI/litellm/blob/main/litellm/proxy/_types.py#L4239
- LiteLLM issue（透传把代理 Authorization 泄漏上游的前车之鉴）：https://github.com/BerriAI/litellm/issues/32202 、https://github.com/BerriAI/litellm/issues/19618
- NGINX ngx_http_proxy_module（默认 `proxy_set_header Host $proxy_host; Connection close;`，不默认透传客户端 Host/Connection；XFF 需显式 `$proxy_add_x_forwarded_for`；空值删头）：https://nginx.org/en/docs/http/ngx_http_proxy_module.html
- Envoy HTTP header 文档（出站恒设 :scheme/:method/:path；Host 不可用 request_headers_to_add 修改、走 host_rewrite；外部请求 x-request-id 默认重新生成除非 preserve_external_request_id；XFF 需 trusted hops 才信）：https://www.envoyproxy.io/docs/envoy/latest/configuration/http/http_conn_man/headers
- APISIX ai-proxy（Apache APISIX AI 代理插件；上游头转发行为文档，作为 LLM 网关同类参考）：https://apisix.apache.org/docs/apisix/ai/ai-proxy/
- new-api issue（自定义 upstream request id 响应头名/本地 request_id 转发，国内中转站实践参考）：https://github.com/QuantumNous/new-api/issues/6512
