# Spec: /v1/responses 与 /v1/messages 原样透传 + 虚拟模型接口类型

Status: ready-for-agent
来源: REQUIREMENTS.md（grilling 两轮拍板 + 10 项自决设计点，用户已确认共识）

## Problem Statement

部分 agent 客户端只会说原生协议：要么调 OpenAI Responses 接口，要么调 Anthropic Messages 接口，且不会在多轮对话中回传 `reasoning_details`。现有网关只提供 OpenAI chat/completions 入口，靠协议转换 + reasoning_details 无损透传来保持思考块完整；这类客户端不回传 reasoning_details，转换链路对它们不成立（思考签名断链、上游拒收或降智），导致这些 agent 无法正常使用网关。

## Solution

新增 `/v1/responses` 与 `/v1/messages` 两个原样透传端点，并给虚拟模型引入「接口类型」维度：

- 虚拟模型类型：OpenAI Compatible（新建默认）/ Responses / Messages / Gemini（保留）/ Full Compatible（历史回填，编号与协议类型对齐 0-4）。
- 端点与类型严格对应：chat/completions 接受 OpenAI Compatible + Full Compatible；/v1/responses 只接受 Responses 类型；/v1/messages 只接受 Messages 类型。枚举编号与协议类型对齐：0=OpenAI Compatible、1=Responses、2=Messages、3=Gemini（保留）、4=Full Compatible。
- 受限类型虚拟模型只能挂本协议成员（Full Compatible 不限），成员协议中途变更时自动从受限类型虚拟模型中移除。
- 新端点请求/响应体不做任何协议转换（仅改写 model 字段），请求头全量透传（黑名单兜底），usage 解析后落现有 request 表。

## User Stories

1. 作为使用 Codex 类原生 Responses agent 的用户，我想把 agent 指向网关的 /v1/responses 并使用 Responses 类型的虚拟模型，使 agent 不回传 reasoning_details 也能正常完成多轮推理对话。
2. 作为使用 Claude Code 类原生 Anthropic agent 的用户，我想把 agent 指向网关的 /v1/messages 并使用 Messages 类型的虚拟模型，使思考签名通过原生协议原样往返而不丢失。
3. 作为网关管理员，我想给虚拟模型选择接口类型（默认 OpenAI Compatible），使新模型默认面向现有 chat/completions 生态、需要原生协议时才显式选择。
4. 作为网关管理员，我想让历史虚拟模型自动归为 Full Compatible，使升级后它们的行为与升级前完全一致。
5. 作为网关管理员，我想在虚拟模型添加成员时只看到本协议的候选成员，使我无法意外拼出协议不匹配的组合。
6. 作为网关管理员，我想让 Full Compatible 虚拟模型的成员添加列表显示所有协议的候选成员，使它能继续聚合任意协议的上游。
7. 作为网关管理员，我想在供应商模型的协议（模型级覆盖或供应商级）中途变更后，自动把它从不再匹配的受限类型虚拟模型中移除，使虚拟模型永远不会带着不兼容成员上线服务。
8. 作为网关管理员，我想让 Full Compatible 虚拟模型在成员协议变更时不被移除成员，使它保持「什么都能接」的聚合语义。
9. 作为网关管理员，我想修改虚拟模型的接口类型并让系统自动清理不再匹配的成员（修改前有确认提示），使类型调整不需要手动清理。
10. 作为使用 /v1/models 做模型发现的客户端，我想只看到 OpenAI Compatible 与 Full Compatible 类型，使列出的模型都能被 chat/completions 正常调用。
11. 作为直接调 /v1/models/{id} 的客户端，我想让 Responses/Messages 专用模型按 404 处理，使模型列表语义与调用语义一致。
12. 作为用 Anthropic SDK 接入的 agent，我想用 x-api-key 头（或 Bearer）完成鉴权，使 SDK 零配置接入 /v1/messages。
13. 作为用 OpenAI SDK 接入的 agent，我想继续用 Authorization Bearer 鉴权 /v1/responses，使 SDK 零配置接入。
14. 作为依赖 anthropic-beta / OpenAI-Beta 等特性头的客户端，我想让这些头原样到达上游，使客户端特性（prompt caching、token-efficient tools 等）不被网关静默吞掉。
15. 作为网关管理员，我想让透传请求的用量（含缓存命中）落到现有 request 指标表，使数据面板完整覆盖新端点流量。
16. 作为网关管理员，我想让新端点的失败/成功也按现有降级策略在成员间 failover，使单成员故障不致整体不可用。
17. 作为调用类型不匹配端点的客户端，我想收到对应协议原生格式的错误响应，使 SDK 的错误处理路径正常工作。
18. 作为网关管理员，我想在数据面板中继续看到新端点产生的请求记录（模型、供应商、token、耗时），使可观测性不因接口升级而出现盲区。

## Implementation Decisions

1. **接口类型枚举与存储**：virtual_model 表新增整数接口类型列，编号与协议类型对齐：0=OpenAI Compatible（新行默认）、1=Responses、2=Messages、3=Gemini（保留值，暂无对应端点）、4=Full Compatible。启动迁移把存量行回填为 4；迁移版本号从 16 号段起编（生产 schema_migrations 残留旧 14/15 号段记录，撞号会被版本守卫静默吞掉）。
2. **端点-类型对应**（严格）：/v1/chat/completions 接受类型 ∈ {0, 4}；/v1/responses 只接受类型 1；/v1/messages 只接受类型 2。不匹配时返回对应协议原生格式错误（/v1/responses 用 OpenAI error 结构，/v1/messages 用 Anthropic type/error 结构），语义为模型不存在/不可用。
3. **成员协议匹配**：受限类型的候选成员按生效协议过滤——模型级协议覆盖优先，其次供应商协议。0 类型←生效协议 0；1 类型←生效协议 1；2 类型←生效协议 2；3 类型不限。Gemini（协议 3）成员只能加入 Full Compatible。
4. **协议变更级联移除**：供应商模型协议（模型级覆盖或供应商级协议）发生变更后，若其所属虚拟模型为受限类型且生效协议不再匹配，硬删该成员行（成员回到未分配状态可重新添加）；Full Compatible 虚拟模型不移除。挂载点覆盖两条路径：模型编辑与供应商协议编辑。
5. **后端保存校验**：虚拟模型创建/更新接口对受限类型校验全部成员协议匹配，不匹配拒绝（添加时）；类型修改引发的级联移除在更新事务内自动执行（前端先行确认提示）。前端候选列表同步按协议过滤，但不信任前端。
6. **透传语义**：新端点请求/响应体不做协议转换，仅把请求体 model 字段从 display_id 改写为成员真实模型 ID；流式与非流式均支持（跟随客户端请求），响应流原样中继。
7. **请求头策略**：新端点全量透传客户端请求头，仅剥离：协议鉴权头（由网关生成上游鉴权头替换，不可覆盖）、hop-by-hop、host/content-length 等既有剥离清单（黑名单始终兜底、写入时拒绝黑名单头）。x-opencode-session 会话亲和回退沿用现有逻辑。
8. **鉴权**：复用现有 api_key 表 Bearer 校验；/v1/messages 额外接受 x-api-key 头作为凭证来源（两者等价，都走同一 api_key 校验）。
9. **failover**：沿用现有降级策略语义——非流式与流式首字节前失败可按策略重试其他成员，流式开始后中断不重试。
10. **用量记录**：从原生响应解析 usage 写入现有 request 表字段，不新增表字段：Anthropic input_tokens/output_tokens→现有字段，input_cache_tokens 按线上既有口径只记 cache_read（cache_creation 是写入不计命中）；Responses input_tokens/output_tokens→现有字段，cached_tokens→input_cache_tokens，reasoning tokens 计入 output_tokens。Anthropic 与 Responses 原生即带 usage，预计无需请求侧注入；实现时验证，确需 opt-in 才注入（对应「include_usage 式操作」的意图是保证 usage 可记录）。
11. **/v1/models 过滤**：列表与单查只返回/匹配类型 ∈ {0, 4} 且启用的虚拟模型。
12. **前端**：虚拟模型表单加接口类型选择器（新建默认 OpenAI Compatible）；成员添加候选列表按当前类型过滤生效协议；修改类型时列出将被级联移除的成员并确认。
13. **复用**：上游 HTTP 客户端、连接池、LB 选路/用量排序、api_key 鉴权、指标落库、failover 骨架全部复用现有实现，不为透传新建请求体解析层。

## Testing Decisions

- 原则：只测外部行为（HTTP 请求/响应、数据库落库结果），不测内部实现细节。
- **缝 A（集成测试 + mock 上游）**：对 /v1/responses、/v1/messages 发真实 HTTP 请求，断言：原样透传（含 model 改写）、类型路由与不匹配拒绝（原生错误格式）、头透传与鉴权头替换、x-api-key 鉴权、流式/非流式、usage 落 request 表、failover。先例：现有四协议转换集成测试（build_authed_app + 本地 mock 上游）。
- **缝 B（虚拟模型集成测试）**：成员协议匹配校验拒绝、协议变更级联移除（两条挂载路径）、Full Compatible 不移除、历史回填迁移、/v1/models 过滤。先例：虚拟模型 CRUD 集成测试。
- **缝 C（纯函数单测）**：Anthropic/Responses 原生 usage JSON → request 字段归一（含缓存、reasoning token、边界/缺失字段）。先例：proxy::convert 单测。

## Out of Scope

- Gemini 原生透传端点。
- 新端点独立模型列表接口。
- request 表新增字段。
- 改动现有 chat/completions 转换链路行为。
- Full Compatible 的成员协议变更移除。
- 请求/响应体的任何协议转换或思考块包装（透传端点不做 reasoning_details 机制）。

## Further Notes

- 端点路径用官方复数形式 /v1/responses（OpenAI）；/v1/messages 同 Anthropic。
- 透传端点不做 store 等字段改写，客户端请求什么就转发什么。
- 代理、超时等上游网络行为与现有转发一致（连接池复用）。
- 实现时验证 Responses API usage 在流式 response.completed 与非流式响应体中的可用性；若某上游需 opt-in 参数，按「最小注入」处理并在工单内记录。
