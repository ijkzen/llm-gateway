# REQUIREMENTS: /v1/responses 与 /v1/messages 原样透传 + 虚拟模型接口类型

> 来源：用户 2026-09-07 原始请求，经 grilling 两轮拍板 + 10 项自决设计点确认定稿。
> 背景：部分 agent 原生调用 OpenAI Responses / Anthropic Messages 接口且不回传
> reasoning_details，导致现有转换链路（思考块无损透传依赖 reasoning_details 回传）不可用。

## 范围

### 1. 虚拟模型接口类型字段

- `virtual_model` 新增接口类型字段，编号与协议类型对齐：
  - `0` = OpenAI Compatible（**新建默认值**）
  - `1` = Responses
  - `2` = Messages
  - `3` = Gemini（保留值，暂无对应端点）
  - `4` = Full Compatible（**历史虚拟模型回填 4**）
- 迁移号从 16 起编（生产 schema_migrations 残留旧 14/15 号段，撞号被静默吞）。
- 接口类型创建后**允许修改**；保存时自动**硬删**不再匹配协议的成员行
  （前端修改类型时先提示确认将级联移除的成员）。

### 2. 端点与类型的严格对应

| 端点 | 接受的虚拟模型类型 |
| --- | --- |
| `POST /v1/chat/completions` | OpenAI Compatible + Full Compatible |
| `POST /v1/responses` | 仅 Responses |
| `POST /v1/messages` | 仅 Messages |

- 类型不匹配 → 对应协议原生格式错误响应（如 Anthropic `type/error` 结构）。
- `GET /v1/models` 与 `GET /v1/models/{display_id}` 只返回 OpenAI Compatible +
  Full Compatible 类型的启用虚拟模型。
- 不为新端点单独提供模型列表接口（不做）。

### 3. 成员协议匹配规则

- 受限类型虚拟模型**只能添加本协议成员**：
  - OpenAI Compatible 类型 → 仅 OpenAI Compat（protocol_type=0）成员
  - Responses 类型 → 仅 OpenAI Responses（=1）成员
  - Messages 类型 → 仅 Anthropic Messages（=2）成员
  - Full Compatible → 任意协议成员
- 成员协议取生效协议：`provider_model.protocol_type` 覆盖优先，否则 `provider.protocol_type`。
- Gemini（=3）成员无原生端点，只能加入 Full Compatible 虚拟模型。
- **中途变更协议**：成员模型（或其供应商）协议变更后不再匹配所属虚拟模型类型时，
  自动**硬删**该 `virtual_model_item` 行（成员回到未分配状态，可重新添加）。
  Full Compatible 虚拟模型的成员变更协议**不移除**。
- 添加成员 API 与前端选择列表同步按协议过滤（非本协议成员不可见/不可加），
  后端保存时仍需校验（不信任前端）。

### 4. 新端点行为（原样透传）

- `/v1/responses`、`/v1/messages` 均**原样透传**到选中成员上游：
  - 仅改写请求体 `model` 字段（虚拟模型 display_id → 成员真实 provider_model_id）。
  - 不做请求/响应体协议转换。
- **请求头全量透传**（黑名单兜底）：剥离鉴权头（换成上游协议鉴权头，网关生成）、
  hop-by-hop、host/content-length 等既有剥离清单；anthropic-beta、OpenAI-Beta 等
  feature 头自然透传。`x-opencode-session` 回退逻辑沿用。
- **鉴权**：`/v1/messages` 同时接受 `x-api-key` 与 `Authorization: Bearer`；
  `/v1/responses` 用 Bearer。均走现有 api_key 表校验。
- **错误响应用对应协议原生格式**（/v1/responses 用 OpenAI error 结构、
  /v1/messages 用 Anthropic type/error 结构）。
- **failover**：沿用现有降级策略语义（非流式/流式首字节前可重试其他成员，
  流式开始后中断不重试）。
- **用量记录**：从原生响应解析 usage 归一写入现有 `request` 表字段
  （不新增表字段）：
  - Anthropic：input_tokens / output_tokens / cache_creation+cache_read → input_cache_tokens
  - Responses：input_tokens / output_tokens / cached_tokens → input_cache_tokens，
    reasoning tokens 计入 output_tokens
  - 懒人备注：Anthropic 与 Responses 原生即返回 usage，预计无需请求侧注入；
    实现时验证，确需 opt-in 才加注入（对应用户「include_usage 注入这样的操作」意图：
    保证 request 表能记录到用量）。
- 流式与非流式都要支持（跟随客户端请求）。

### 5. 前端

- 虚拟模型表单/详情加「接口类型」选择器（新建默认 OpenAI Compatible）。
- 成员添加列表按虚拟模型类型过滤协议（Full Compatible 显示全部）。
- 修改接口类型时提示确认级联移除的成员。

## 非目标（明确不做）

- 不做 Gemini 原生透传端点。
- 不为新端点提供独立模型列表接口。
- request 表不加新字段。
- 不改现有 /v1/chat/completions 转换链路行为。
- Full Compatible 的成员协议变更不移除（仅受限类型移除）。

## 已拍板决策记录

1. Full Compatible **严格只接受对应类型**：新端点不接受 Full Compatible，仅 chat/completions 接受。
2. 新建 OpenAI Compatible 类型**严格限本协议成员**（转换能力只留给 Full Compatible）。
3. 「请求新增用量字段」= 对新接口做 include_usage 式操作保证用量可记录，非加表字段。
4. 接口类型允许修改 + 级联移除不匹配成员。
5. 级联移除方式 = 硬删成员行。
6. 透传头策略 = 全量透传、黑名单兜底。
