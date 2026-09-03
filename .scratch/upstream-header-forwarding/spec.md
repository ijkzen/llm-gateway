# SPEC — 上游 Header 传递修复（转发链路）

Status: 待审批（approval gate）
Date: 2026-09-03
Slug: upstream-header-forwarding
Branch: fix/upstream-header-forwarding
Prereq: `REQUIREMENTS.md`（决策 D1–D4）、`research.md`（规范与行业依据）

---

## 1. Problem Statement

下游请求头在 `/v1/chat/completions` 入口被整体丢弃；网关把「协议鉴权头 / provider custom_header / 框架头」三段按时间顺序 Vec 追加，同名产生**重复值**（`Content-Length`/`Host` 重复属 RFC 9112 framing 违规，`Authorization` 重复语义含糊且可被下游/误配污染）。无剥离清单、无覆盖优先级。

## 2. Solution（概览）

把上游出站头组装重构成**唯一 HeaderMap + 四层 insert（覆盖式）+ 剥离清单**：

```
出站 HeaderMap（唯一表，有序）
  第4层 下游 allowlist 透传（默认 traceparent/tracestate）   ← 最先 insert
  第3层 provider custom_header                               ← insert 覆盖第4层
  第2层 协议鉴权/必需头（authorization/x-api-key/…）          ← insert 覆盖第3/4层（D3）
  第1层 框架头 Host/Content-Type/Accept/Content-Length        ← 最后强制 insert（上游侧）
```

剥离清单（下游头在进入 allowlist 前整体检查，命中即丢弃）：见 `REQUIREMENTS.md` §4 与 `research.md` §2。

透传头从**下游 `HeaderMap` 提取**：入口 handler 读取下游头 → 过滤出 allowlist 命中的项（first-wins、单值）→ 传入 `forward_chat` → `build_upstream_call`。非 allowlist 下游头一律不进上游（现状保持）。

## 3. 调用链与改动锚点

### 现状（worktree HEAD == main d4c3191，已核实行号）

- 入口：`src/routes/openai_compat.rs:27-33` — `chat_completions` handler，签名仅 `State`/`Extension(AuthedApiKey)`/`Json(Value)`；无 `HeaderMap`。
- 唯一调用 `forward_chat`：`openai_compat.rs:32`。
- 转发核心：`src/proxy/mod.rs:763` `forward_chat(state, api_key, client_body)`；失败循环内 `src/proxy/mod.rs:949` 调 `build_upstream_call`。
- 头组装中心：`src/proxy/mod.rs:471-551` `build_upstream_call(member, chat, client_stream, api_key)`；`471-581` 区含 `auth_headers` 生成 + `merge_custom_headers`（540 行）。
- custom_header 追加：`src/proxy/mod.rs:564-580` `merge_custom_headers`（parse 失败静默、非字符串值跳过、**append**）。
- 协议枚举：`src/proxy/mod.rs:46-64` `Protocol`。
- 调用载体：`src/proxy/mod.rs:543` 构造 `UpstreamCall`；结构 `src/proxy/upstream.rs:403-409`（`headers: Vec<(HeaderName,HeaderValue)>`）。
- 框架头/发送：`src/proxy/upstream.rs:502-528` `send_upstream_request`：`Builder` 预置 `HOST`/`Content-Type`/`accept`/`Content-Length` 后逐条 `.header()` 追加 `call.headers`。
- 复用构造器的第二调用点：`src/proxy/mod.rs:1682` `test_model` 内 `build_upstream_call`。
- `UpstreamCall` 字面量构造：`tests/upstream_pool_integration.rs:58,213,241,251`（改字段会波及）。

### 计划改动

**A. `src/routes/openai_compat.rs`**
- `chat_completions` 增加 `headers: axum::http::HeaderMap` 提取（parts extractor，须放在 `Json` 之前，body extractor 后置）。
- `forward_chat(&state, api_key, body)` → 增加参数，传入过滤后的透传头子集。

**B. `src/proxy/mod.rs`**
- 新增「剥离 + allowlist + 分层覆盖」的头组装辅助（纯函数，便于单测）：
  - 剥离清单常量（`RESERVED_OUTBOUND_HEADERS` / hop-by-hop 集合 / 凭据名集合）。
  - `select_forwardable_headers(&downstream_headers, allowlist) -> Vec<(HeaderName,HeaderValue)>`：只放行 allowlist 命中项，first-wins、单值。
  - 默认 allowlist 常量：`traceparent`、`tracestate`。
- `build_upstream_call` 重构：
  - 签名增加 `downstream_forwarded: &[(HeaderName, HeaderValue)]`（或直接收已过滤子集）。
  - 组装顺序：第4层透传 → 第3层 `custom_header`（`insert` 覆盖）→ 第2层协议鉴权/必需头（`insert` 覆盖，D3）→ 结果放 `UpstreamCall.headers`。
  - custom_header 与协议鉴权/必需头冲突时 `tracing::warn!`（记 request_id/协议，不记值）。
  - `merge_custom_headers` 改为 `insert` 语义（不再 append 同名）。
- `forward_chat` 签名增加透传头参数；从 handler 接收后直接传给 `build_upstream_call`。
- `test_model`（`src/proxy/mod.rs:1682`）传空透传子集（无下游头），自动继承修复。

**C. `src/proxy/upstream.rs`**
- `UpstreamCall.headers` 语义从「追加表」改为「**已最终确定的唯一表**」；`send_upstream_request` 里框架头（`HOST`/`Content-Type`/`accept`/`Content-Length`）改为**在追加 `call.headers` 之后最后强制 `insert`**（或等价保证），杜绝 `call.headers` 内同名项造成重复。
- 文档注释说明：`call.headers` 不得含框架头同名项（由上层保证），发送端兜底强制覆盖。

**D. 测试**（详见 §Testing；新增到 `tests/proxy_integration.rs` + `src/proxy/mod.rs` 单测 + `tests/upstream_pool_integration.rs` 字面量随结构补字段）。

### 不变量 / 优先级（与 REQUIREMENTS §4 一致）

框架头(1) > 协议鉴权/必需头(2) > provider custom_header(3) > 下游 allowlist 透传(4)。`custom_header` 不得覆盖第 2 层（D3）；框架头永远最后强制（第 1 层）。

## 4. 语义头处理（本特性内）

| Header | 处理 |
| --- | --- |
| `traceparent` / `tracestate` | 默认 allowlist 透传；原样、first-wins、单值；非法/超长忽略（不 400）。 |
| `Authorization` | 下游值只用于入口鉴权；**绝不出站**。出站 `authorization` 只由第 2 层生成（解密 provider key）。 |
| `Cookie` / `Proxy-Authorization` / `x-api-key` / `x-goog-api-key` 等凭据名 | 剥离清单；绝不出站。 |
| `Host` / `Content-Length` / `Content-Type` / `Accept` | 剥离清单（下游值不出站）；由第 1 层重新生成。 |
| `Connection` / `Transfer-Encoding` / `Keep-Alive` 等 hop-by-hop | 剥离清单；绝不出站。 |
| `X-Forwarded-*` / `Forwarded` / `Via` 等链路头 | 剥离清单；绝不出站。 |
| 其余任意 `X-*` / `User-Agent` / `OpenAI-*` / `anthropic-*` | 默认**不**在 allowlist → 不透传（现状保持）；未来按常量扩展 allowlist。 |

## 5. Testing Decisions

### 5.1 单测（`src/proxy/mod.rs` 内 `#[cfg(test)]`，复用现有 `provider()`/`model()` 构造器 1798-1854）

1. 剥离：下游带 `Authorization`/`Cookie`/`Host`/`Content-Length`/`Connection`/`X-Forwarded-For` → 不出现在出站头。
2. 透传 allowlist：下游带 `traceparent`/`tracestate` → 原样单值出站；带任意非 allowlist `X-*` → 不出站。
3. 优先级：custom_header 同名 `authorization`（Anthropic 场景为 `x-api-key`/`anthropic-version`）→ 出站以第 2 层为准、无重复、触发 warn；custom_header 的普通 `X-A` → 生效。
4. 无重复：对任意 key 断言 `.get_all().len() == 1`。
5. custom_header JSON 非法 / 非对象 / 非字符串值：静默跳过（现语义保持）。

### 5.2 集成测试（`tests/proxy_integration.rs`）

mock 上游从「只读 body」升级为「可读 headers」（参照 `tests/provider_usage_integration.rs:349-356` 的 axum `HeaderMap` 读取样板）。建议矩阵（每格一个断言，见 `research.md` §10.2）：

| # | 场景 | 断言 |
| --- | --- | --- |
| 1 | 下游带 `traceparent`/`tracestate` | 上游收到原样 traceparent/tracestate 单值 |
| 2 | 下游带 `Authorization: Bearer lg-xxx` | 上游 `authorization` == `Bearer <provider key>`，**不含** lg-xxx |
| 3 | 下游带 `Cookie`/`Proxy-Authorization` | 上游无二者 |
| 4 | 下游带 `Host: evil.example` | 上游 `Host` == member base_url authority |
| 5 | 下游带 `Content-Length: 1` / `Transfer-Encoding: chunked` / `Connection: keep-alive, x-foo` | 上游 CL == 真实 body 长；无 TE/Connection/x-foo |
| 6 | custom_header 配 `{"authorization":"Bearer x","anthropic-version":"2023-06-01","X-A":"b"}`（Anthropic） | 上游 `authorization`→用 provider key 值；`anthropic-version`→2023-06-01；`X-A`→b；无重复行 |
| 7 | 下游带 `x-request-id`（非 allowlist） | 上游无 `x-request-id`（默认不透传） |
| 8 | 流式 & 非流式 | `Accept` 恒 `application/json, text/event-stream`；`Content-Type: application/json` |
| 9 | failover 两次出站 | 每次出站头一致（同一下游头快照，不串） |
| 10 | 四协议各一次 | 各协议鉴权头名称正确、只有一份 |
| 11 | 下游无透传头 | 出站头 == 框架 + 鉴权 + custom（最小集回归） |
| 12 | 客户端带 `traceparent` 走 `/test`（`test_model`） | 上游无 traceparent（test_model 透传子集为空） |

### 5.3 `UpstreamCall` 字面量（`tests/upstream_pool_integration.rs:58,213,241,251`）
若结构体字段语义调整，同步更新这些测试，确保既有连接池测试语义不回归。

## 6. 兼容性 / 回归风险

- **行为变化对既有客户**：无下游透传时出站头集合 == 现状（框架 + 鉴权 + custom_header），仅消除重复行 → 对依赖「custom_header 能追加同名重复头」的客户有微小行为变化，但该用法本就是缺陷（重复头非预期）。默认透传 trace 头是新行为（下游带 traceparent 时上游会收到），观测性收益 > 风险。
- **HTTP/1.1 出站**：`Connection` 剥离在出站端由应用保证（hyper 出站不剥离 hop-by-hop，已核 `role.rs` 编码路径）。SSE 响应侧手工 `connection: keep-alive` 保持现状，不在本特性改动。
- **测试扰动**：`tests/upstream_pool_integration.rs` 4 处 `UpstreamCall` 字面量 + mock 升级。

## 7. 参考

- `research.md` §1–§10（RFC 9110/9112、W3C Trace Context、LiteLLM 先例、完整来源）
- `REQUIREMENTS.md` §5（决策理由）
