# REQUIREMENTS — 上游 Header 传递修复（转发链路）

Status: 待审批（approval gate）
Date: 2026-09-03
Slug: upstream-header-forwarding
Branch: fix/upstream-header-forwarding（worktree: `llm-gateway-upstream-header-forwarding`）
调研: 见同目录 `research.md`

## 1. 需求原话（用户）

> 如果一个 provider 有自定义 header，与此同时下游发送的请求也有自己的 header，谁的优先级高，最终发给上游的请求是谁的 header？

→ 追溯代码后确认：当前下游 header 在 `/v1/chat/completions` 入口被整体丢弃，**不存在下游 header 与 provider custom_header 的优先级问题**；真正缺陷是网关自身 header 组装存在三类问题：
1. provider `custom_header` 与协议鉴权头、与框架头（`Host`/`Content-Type`/`Accept`/`Content-Length`）可**同名重复**（`HeaderValue` 追加语义 → 上游收到多行重复头，`Content-Length`/`Host` 重复属 framing 违规，`Authorization` 重复语义含糊）。
2. 下游 `Authorization: Bearer <lg-网关key>` 在入口被用于鉴权，但**除鉴权外全部下游头被丢弃**：`traceparent`/`tracestate`/`User-Agent`/`x-request-id`/`OpenAI-*`/自定义 `X-*` 一律不进上游。
3. 网关无「层模型」：无剥离清单、无覆盖优先级，只有按时间顺序的 Vec 追加。

## 2. 范围 Scope（已拍板）

- **链路**：仅 `/v1/chat/completions` 转发链路（`chat_completions` → `forward_chat` → `build_upstream_call` → `UpstreamCall` → `send_upstream_request`）。`test_model` 因复用 `build_upstream_call` 自动继承「剥离 + 三层覆盖」修复，但它无下游头、不透传（见 §5.3）。
- **不做**：模型刷新（`src/provider_model/refresh.rs`）与用量抓取（`src/usage/http.rs`）链路的同类 header 处理——保持现状，跨链路差异记为已知问题。
- **不做**：上游响应 header 透传（`Set-Cookie`/限流头等）；`baggage` 透传；`OpenAI-Organization/Project`/`anthropic-organization-id` 归属头透传；BYOK/`forward_llm_provider_auth_headers`；Idempotency-Key；头部大小 64KiB 硬上限校验。
- **前端无改动**：本特性是纯后端 header 组装重构 + 透传能力。

## 3. 已拍板决策（Approved Decisions）

| # | 决策 | 结论 |
| --- | --- | --- |
| D1 | 修复形态 | **修复 + 可选透传（trace/allowlist）**：重构为三层覆盖模型，并提供下游头 allowlist 透传能力。 |
| D2 | 透传默认值 | **默认透传 trace 头**：`traceparent`/`tracestate` 原样透传出站。 |
| D3 | custom_header 冲突 | **默认禁覆盖**：provider `custom_header` 不得覆盖网关生成的协议鉴权/必需头（`authorization`/`x-api-key`/`x-goog-api-key`/`anthropic-version`），冲突时以协议头为准并记告警日志；不加 per-provider 覆盖开关（YAGNI，见 §6）。 |
| D4 | 同步范围 | **仅转发链路**，不同步到刷新/用量链路。 |

## 4. 目标行为（验收口径）

上游收到的出站请求头 = **唯一的 HeaderMap**，分层 `insert`（覆盖式）构建，顺序从低优先级到高优先级：

1. **第 4 层 下游 allowlist 透传**（默认：`traceparent`、`tracestate`；原样、单值、first-wins）。
2. **第 3 层 provider `custom_header`**（JSON 对象，字符串值）——`insert` 覆盖第 4 层同名项。
3. **第 2 层 协议鉴权/必需头**（按协议：OpenAI 系 `authorization: Bearer <key>`；Anthropic `x-api-key` + `anthropic-version: 2023-06-01`；Gemini `x-goog-api-key`）——`insert` 覆盖第 3/4 层同名项（D3：custom_header 不得覆盖协议头）。
4. **第 1 层 框架头** `Host`/`Content-Type`/`Accept`/`Content-Length`——最终无条件 `insert` 覆盖一切同名项。

**不变式**（均有测试）：
- 出站请求**零同名重复**（对剥离清单内任意 key，`.get_all().len() == 1`）。
- 下游 `Authorization`/`Cookie`/`Proxy-Authorization`/`x-api-key`/`x-goog-api-key` 等凭据名**绝不**到达上游。
- 下游 `Host`/`Content-Length`/`Content-Type`/`Accept` 等框架名**绝不**到达上游。
- 下游 `Connection`/`Transfer-Encoding`/`Keep-Alive` 等 hop-by-hop **绝不**到达上游。
- 下游无任何透传头时，出站头 == 框架 + 协议鉴权 + custom_header（最小集回归）。

## 5. 决策记录与理由（grilling 结论）

- **为什么默认禁 custom_header 覆盖协议鉴权头（D3）**：否则管理员误配同名 `authorization`/`x-api-key` 会让整条链路凭据变成手填值（可能是旧 key/明文泄露），而系统明明配了加密存储的 provider key。覆盖应是显式功能而非「谁 append 在后谁赢」的静默行为。当前没有真实场景需要覆盖协议鉴权头（没有 BYOK、没有按 provider 冒认 key 的需求），故连开关也不做，规则写死 + 告警。
- **为什么默认透传 traceparent/tracestate（D2）**：W3C Trace Context 要求接收方把 `traceparent` 原样发往出站请求；内容非凭据。透传只把下游 trace 树暴露给 provider，属可接受的观测性收益。`tracestate` 仅在透传 `traceparent` 时透传（未改 traceparent 则不改 tracestate）。
- **为什么透传默认集只含 trace 两件 + 允许白名单扩展（D1）**：`x-request-id`/`baggage`/自定义 `X-*` 的透传语义与隐私权衡更重（x-request-id 与内部 request_id 冲突、baggage 可带 PII），按行业建议默认保守。allowlist 以 `HeaderName` 常量注入，未来要放开只加常量即可；本特性暂不做 DB/设置项（YAGNI，见 §6）。
- **为什么仅转发链路（D4）**：刷新/用量链路是 reqwest 且各自独立组装 custom_header，改动会扩散到用量 fetcher 等敏感路径；它们与「下游头透传」无关（无下游概念）。跨链路 custom_header 语义差异（proxy 追加 vs reqwest 覆盖）已长期存在且不影响正确性，记为已知问题。
- **为什么 test_model 只继承修复不透传**：`test_model` 无下游请求头（`/api/providers/.../test` 是管理面手动触发），allowlist 为空即自然不透传；但它复用 `build_upstream_call`，因此同样获得「custom_header 不覆盖协议头 + 无重复头」修复。

## 6. 裁剪（ponytail 裁定 / Out of Scope）

- 不做 DB/热更设置项；透传默认集 + allowlist 以代码常量表达（本仓库设置面为 `/api/settings` 键值热更，为两个固定头引入设置项过重）。
- 不做 per-provider 的「允许 custom_header 覆盖协议鉴权头」开关（无场景）。
- 不做上游响应头透传 / 响应 `Set-Cookie` 例外处理（本网关出站响应头固定，provider 不设 Set-Cookie）。
- 不做 64KiB 头部上限校验（下游可发任意大 header 是既有事实，出站剥离后并不放大风险）。
- 不做 UA 生成/`Accept-Encoding` 等新协商头（现状固定头已够；UA 透传按 research 建议不做，但网关也暂不新增自报 UA——避免行为变化）。
- 不做 `baggage`、归属头、Idempotency-Key、BYOK。

## 7. 验收口径

- 后端 `cargo fmt`/`cargo clippy -D warnings`/`cargo test --all-targets` 全绿。
- 集成测试断言见 `spec.md` §Testing（14 项矩阵 + 四协议鉴权基线）；`merge_custom_headers`/新剥离逻辑补单测。
- 行为回归：下游不带任何透传头时出站头集合 == 现状（框架 + 鉴权 + custom_header），无重复行。
- `test_model` 功能不回归（复用构造器，仅头组装语义修正）。
