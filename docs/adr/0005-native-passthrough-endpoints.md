# 0005 — 原生透传端点与虚拟模型接口类型

## Status

accepted

## Context

网关 /v1 长期只对外提供 `POST /v1/chat/completions`（OpenAI Compat 转换聚合），Anthropic/Responses 原生客户端只能经协议转换访问，转换路径对推理模型有结构性限制：Responses 推理要求请求侧恒带 `include: ["reasoning.encrypted_content"]`、工具轮必须回传加密 reasoning item（`store:false`），Anthropic 思考块经转换后工具轮无法回传签名导致降智或 400。此外转换层把上游语义折算成 chat/completions，原生错误格式、usage 字段差异与 SSE 事件结构都会丢失。ZCode 等客户端对 `opencode.ai` 上游已按协议原生接入，需要一个不经转换的直通面。

## Decision

1. 新增 `/v1/messages`（Anthropic Messages）与 `/v1/responses`（OpenAI Responses）原生透传端点，`virtual_model` 新增 `interface_type`（迁移 23）：0=OpenAI Compat / 1=Responses / 2=Messages / 3=Gemini 保留 / 4=Full Compatible。端点与类型严格对应：`chat/completions` 接 {0,4}、`/v1/responses` 仅 1、`/v1/messages` 仅 2；`/v1/models` 列表与单查只出 {0,4}。
2. 历史回填：存量虚拟模型行回填 `interface_type=4`（Full Compatible——升级前所有虚拟模型都是「任意协议成员 + chat/completions 转换」语义），新建默认 0；Gemini(3) 为保留值，拒绝创建。
3. 成员资格按生效协议匹配（模型级覆盖 > 供应商）；协议变更/接口类型变更级联硬删不匹配成员（Full Compatible 豁免，保持转换聚合语义）。
4. 原生透传只改写 `model` 字段，SSE 原始字节中继不重帧；`/v1/messages` 支持 `x-api-key` 鉴权与 Anthropic 原生错误格式；usage 旁路扫描落现有 request 表字段（缓存只算 read）。原生路径与 chat/completions 共用虚拟模型成员选路、用量感知排序与 failover（`proxy::forward_native`，成员尝试循环与 chat 合一）。

规格与拆票见 `.scratch/v1-native-endpoints/`。

## Consequences

- Anthropic/Responses 原生客户端可零转换直连，思考块、原生错误格式与 SSE 事件结构不再折算丢失；Responses 推理模型满足官方 reasoning 回传要求。
- 端点 × 类型双重约束杜绝错配；新增端点/类型只需在类型表加值。
- `/v1/models` 对外口径随类型过滤，原生客户端只看到自己能调的虚拟模型。
- 存量虚拟模型以 Full Compatible 回填，升级后行为不变；Full Compatible 继续承载跨协议成员聚合这一原有能力。
