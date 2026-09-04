# SPEC — 模型单独选择协议

## Problem Statement

llm-gateway 的 `/v1` 转发支持四种上游协议（OpenAI Compatible / OpenAI Responses / Anthropic Messages / Gemini），转换器与出站 URL 全部按运行时 `Member.protocol` 分发。但协议目前**只存在于供应商层**（`provider.protocol_type`）：同一供应商下所有模型（provider_model）必须共用同一个协议，无法在模型粒度表达差异。

实际使用中存在「同一供应商（或同一网关地址）下的不同模型走不同协议」的场景——例如一个多协议网关用共享 key 同时暴露 OpenAI 兼容与 Anthropic 两条链路，或某个特殊模型只有某一种协议的接入方式。当前架构无法配置，必须把协议下放到模型层。

## Solution

为 `provider_model` 增加**可空列** `protocol_type`：`NULL` = 跟随供应商协议（默认），`0..=3` = 显式覆盖为该协议（枚举含义与供应商一致）。转发与模型测速时按「模型覆盖（非空）→ 供应商协议」解析最终协议；converter 层零改动（它已按 `member.protocol` 分发）。前端仅在供应商模型详情弹窗增加协议编辑（下拉：跟随供应商 + 四种协议）与只读展示（生效协议 + 来源）。

## User Stories

1. 作为管理员，我希望给某个供应商模型单独指定一个协议（OpenAI Compatible / OpenAI Responses / Anthropic Messages / Gemini），这样该模型向上游转发时走我指定的协议链路，而不必与供应商其它模型共用协议。
2. 作为管理员，我希望模型没有单独指定协议时请求回落供应商协议，这样大多数模型无需逐条配置，默认行为与现状一致。
3. 作为管理员，我希望新建/批量导入/刷新候选的模型默认跟随供应商协议，这样添加流程保持简单，协议是后续一次性配置。
4. 作为管理员，我希望在模型详情弹窗的编辑表单里通过下拉选择「跟随供应商」或某一种具体协议，这样配置入口直观。
5. 作为管理员，我希望模型详情弹窗的只读视图显示该模型**生效**的协议及其来源（自定义则协议名，跟随则「跟随供应商 + 供应商协议名」），这样能一眼识别转发实际使用的协议。
6. 作为管理员，我希望模型单独选了协议后，`/v1` 转发自动按该协议组装请求体并调用对应上游接口（如 Responses 走 `/responses`、Anthropic 走 Anthropic builder，URL 依协议补版本段），这样无需任何额外手工操作。
7. 作为管理员，我希望模型测速（详情弹窗「测试」按钮）与转发使用相同的协议解析规则，这样测试结果与真实转发路径一致。
8. 作为管理员，我希望提交非法协议值（超出 0..=3 的非空值）时收到 400 错误，这样脏数据进不了库。
9. 作为管理员，我希望模型协议设置刷新页面（重拉数据）后依然保持，这样配置持久不丢。

## Implementation Decisions

### 数据层

- **迁移 20**：`provider_model` 新增可空列 `protocol_type integer`（无默认值，存量行自动为 NULL → 跟随供应商，无需回填）。
  - 新库由 `create_table_from_entity` 建表自动带列；历史库在 `src/db.rs` 用 `column_exists` 检测 + 单次 `ensure_migration(20, ...)` 兜底 ALTER（沿用 Migration 19 多语句守卫写法与注释约定）。
  - 版本号**用 20**（14/15 被生产库残留的旧 lg-proxy 方案占用，16 起编号，19 已被模型级代理占用）。
- `src/entity/provider_model.rs`：`Model` 增加 `protocol_type: Option<i32>`（SeaORM 可空列）。

### 转发管线（src/proxy/mod.rs）

- `load_members` 组装 `Member.protocol` 时：`model.protocol_type.or(Some(p.protocol_type))` → `Protocol::from_i32`。即模型覆盖非空用模型值，否则回落供应商。**不抽 `resolve_protocol` helper**（一两行 Option 逻辑，两处调用点各自内联，与 ponytail 裁剪一致）。
- `Member` 结构、Converter 分发（`build_upstream_call` / `dispatch_success` / URL 构造）、failover、LB 均零改动。
- `test_model`（测速）组装时套用同一规则，与转发一致。

### API 契约（src/routes/provider_models.rs）

- `ProviderModelResponse` 增加 `protocol_type: Option<i32>`（camelCase `protocolType`，`null` = 跟随）。
- `UpsertProviderModelRequest` 增加 `protocol_type: Option<i32>`（`#[serde(default)]`，camelCase `protocolType`）。
- 校验：`protocol_type` 非空时须在 `0..=3`（复用或仿写 `routes/providers.rs` 的校验；本处仅有协议单字段，就地 `matches!` 即可），非法返回 400 中文错误。
- 创建 / 批量创建 / 更新赋值路径（ActiveModel 构造处）写入该字段；批量导入候选与手动添加缺省 `NULL`。

### 前端（web/src/components/provider-models/ProviderModelDetailDialog.tsx）

- 表单 schema 增加 `protocolType: number | null`，默认 `null`（跟随供应商）。
- 编辑态：协议下拉，选项 = 「跟随供应商（null）」+ 复用 `PROTOCOL_TYPES`（0..3），可配 `ProtocolIcon`；新增 i18n 键「跟随供应商」。
- 只读态：新增协议行，显示生效协议与来源（自定义 = 协议名；跟随 = 「跟随供应商（{供应商协议名}）」）。参照 `ProviderProxyRow` 的继承来源展示；弹窗需接收 `providerProtocolType` prop（调用方 `ProviderModelSection` 已有该数据）。
- `use-provider-models.ts`：`ProviderModel` 接口与 `ProviderModelPayload` 增加 `protocolType: number | null`。
- i18n：`zh-CN` 新增「跟随供应商」键（`providers.protocol.followProvider` 风格）。

## Testing Decisions

- **原则**：只测外部行为（出站请求形态、组装出的成员协议、API 校验），不测实现细节。
- **测试缝 1 — 转发集成（tests/proxy_integration.rs）**：扩展既有四协议转换测试：seed 供应商 protocol=2（Anthropic）+ 成员模型协议覆盖=1（Responses），断言 mock 上游收到的出站请求按 Responses 形态（URL 路径 + body 形态）；再覆盖「模型未覆盖 → 回落供应商协议」回归。仿照现有 `anthropic_non_stream_converts...` / `responses_final_output...` 的 mock 捕获断言模式。
- **测试缝 2 — 模型 CRUD + 组装（tests/provider_models_integration.rs）**：
  - CRUD roundtrip：创建/更新携带 `protocolType`、响应回显、`null` 缺省、非法值（如 4）400 拒绝。仿照 `test_model_proxy_crud_roundtrip` / `test_model_proxy_validation_errors`。
  - 组装断言：供应商协议 X + 模型覆盖 Y → `load_members` 出的成员 protocol 为 Y；模型 `NULL` → X。可用单元测试（`src/proxy/mod.rs` 既有 `test_model` 单元测风格）或集成层断言。
- 前端不做组件测试（用户裁掉）；由 `pnpm lint` + `tsc` + 既有 vitest 全量保障。

## Out of Scope

- converter 层（`src/proxy/convert/*`）零改动；协议分发之上的一切（failover / LB / 用量排名 / 熔断）不受影响。
- 刷新候选 / 批量导入 UI 不支持逐条选协议（默认跟随，添加后进详情弹窗编辑）。
- 虚拟模型成员列表、模型三级页 overview 等其它界面不展示模型生效协议（用户明确：只改详情弹窗）。
- 不做「模型协议与供应商能力匹配」的警告/预检；选错协议按普通转发失败处理（failover 兜底）。
- 用量抓取 / 用量缓存 / provider 路由不感知模型协议（协议字段只影响转发与测速）。

## Further Notes

- 手动测速与转发的协议解析必须同一条规则，避免「测速结果与真实转发不一致」（与模型级代理上线时的经验一致）。
- 存量数据全部 NULL → 自动跟随供应商，上线零回填、零行为变化；这是本功能「默认可逆」的关键。
- 生产库迁移注意：14/15 号段废弃占位，16 起编号，本功能用 20；部署后 PRAGMA 验证列存在。