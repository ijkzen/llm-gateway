# REQUIREMENTS — 模型单独选择协议

## 需求（用户原话）

模型单独选择协议：OpenAI compatible、OpenAI Response、Anthropic message、Gemini；在供应商模型弹窗的详情页面，支持模型单独去选择一个协议。然后你需要去自动去调用对应协议的接口。

## 背景（现状）

- 协议目前**只存在于供应商层**：`provider.protocol_type`（i32，0=OpenAI Compatible / 1=OpenAI Responses / 2=Anthropic Messages / 3=Gemini）。
- 转发时 `load_members`（`src/proxy/mod.rs`）从 provider 行读出 `protocol_type` 组装运行时 `Member.protocol`，converter 分发（`build_upstream_call` / `dispatch_success`）全部只读 `member.protocol`。
- `provider_model` 表无协议字段；model 与虚拟模型成员 1:1（`model_id` 全局唯一，migration 6）。
- 手动测速（`POST /api/providers/{id}/models/{model_id}/test`）同样从 provider 行取协议。

## 范围（Scope）

1. `provider_model` 新增**可空列** `protocol_type`：`NULL` = 跟随供应商协议（默认）；`0..=3` = 显式覆盖为该协议（与 provider 同枚举含义）。新建/批量导入/刷新候选默认 `NULL`。
2. 生效解析优先级：**模型协议（非空）→ 供应商协议**。`load_members` 与 `test_model` 两处组装时应用，其余转发链路（converter 分发、URL 构造、流式转换）**零改动**（它们已按 `member.protocol` 分发）。
3. CRUD：创建/更新/响应携带 `protocolType`（camelCase，`null` 表示跟随）；校验：非空时须在 `0..=3`。
4. 前端：只改供应商模型详情弹窗（`ProviderModelDetailDialog`）：
   - 编辑态：协议下拉（「跟随供应商」+ 四种协议），复用 `PROTOCOL_TYPES` / `ProtocolIcon` / `providers.protocol.*` i18n 键；新增「跟随供应商」文案键。
   - 只读态：显示生效协议（含来源——自定义则显示协议名，跟随则显示「跟随供应商」+ 供应商协议名）。参照 `ProviderProxyRow` 的继承来源展示。
5. 迁移编号 **20**，column_exists 守卫（参照 migration 19），存量行自动回落供应商协议，无需回填。

## 裁剪（ponytail 裁定，均为非目标）

- **不新增 `resolve_protocol` helper**：一两行 `Option` 逻辑，在两个组装点各写一处即可，不抽函数。
- **不改 converter 层**：`convert/`、`build_upstream_call`、`dispatch_success`、URL 构造全部读 `member.protocol`，无需触碰。
- **不改刷新候选 / 批量导入 UI**：新模型默认跟随供应商，添加后到详情弹窗编辑。
- **不加其它页面展示**：虚拟模型成员列表、模型三级页 overview 都不改（用户明确：只改详情弹窗）。
- **不改用量检查 / 用量缓存 / provider 路由**：协议字段只影响转发与测速。
- **不做协议冲突警告 / 校验协议与供应商能力匹配**：超出需求；选错协议按普通转发失败处理（failover 兜底）。

## 决策记录（grilling 拍板）

- **默认语义**：跟随供应商，可单独覆盖（可空列 NULL=跟随）——用户已确认。
- **展示范围**：只改供应商模型详情弹窗，其它页面不动——用户已确认。
- 设计共识（用户确认）：落点 provider_model 可空列；生效优先级模型→供应商；只读态显示生效协议与来源；迁移 20 带守卫。

## 迁移注意（既有知识）

- `schema_migrations` 生产库残留旧 lg-proxy 方案的 14/15 号段，**新迁移从 16 起编号，本功能用 20**。
- 新列 `NULL` 默认，存量模型行自动跟随供应商协议，无需回填；新库由 `create_table_from_entity` 建表自动带列。

## 验收口径

- 模型协议为 `NULL` 的成员：转发/测速走供应商协议（回归现有行为）。
- 模型协议设为 `1`（OpenAI Responses）而供应商为 `2`（Anthropic）的成员：转发按 Responses converter 组装并走对应 URL，测速同规则。
- 编辑弹窗保存协议后回到只读态显示生效协议与来源；刷新页面（重拉数据）后选择保持。
- 非法协议值（如 4 或 -1 非空值）被 400 拒绝。