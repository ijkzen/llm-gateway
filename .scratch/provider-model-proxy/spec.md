# SPEC — 供应商模型级网络代理

## Problem Statement

供应商可以单独配置 HTTP 网络代理（provider 级 `proxy_enabled`/`proxy_addr`，已上线），但虚拟模型成员指向的**供应商模型**（provider_model）本身不能单独配置代理。实际使用中，同一供应商的不同模型可能需要对不同网络路径（例如某模型走内网、另一模型走代理），或者模型需要覆盖供应商的代理设置（模型直连、供应商走代理）。当前只能按供应商统一代理，无法在模型粒度表达。

另外，用量检查与刷新模型列表的上游请求（`src/usage/`、`src/provider_model/refresh.rs`）只关心供应商级代理，模型级代理不应影响这两条路径。

## Solution

为 `provider_model` 增加与 provider 同构的模型级代理字段 `proxy_enabled` + `proxy_addr`（`http://` 开头、无认证）。转发 `/v1` 请求与模型测速（`test_model`）时按「模型级 → 供应商级 → 直连」的优先级解析最终代理；用量抓取与模型刷新保持只认供应商代理（不改）。前端模型「编辑」表单支持开关+地址，只读详情增加「网络代理」展示行。

## User Stories

1. 作为管理员，我希望给某个供应商模型单独开启网络代理，这样它向上游发请求时走我指定的代理，而不是沿用供应商的代理或直连。
2. 作为管理员，我希望模型开了代理但供应商也开了代理时，请求走模型的代理（模型优先），这样个别模型可以覆盖供应商的代理设置。
3. 作为管理员，我希望模型没开代理时请求回落到供应商代理，这样只需在模型层配置少量差异，不必每个模型重复配置。
4. 作为管理员，我希望模型和供应商都没开代理时请求直连，这样默认行为与现状一致，不引入意外代理。
5. 作为管理员，我希望在模型详情弹窗的只读视图看到该模型是否走网络代理、走哪个地址，这样能一眼识别模型的实际网络路径。
6. 作为管理员，我希望在模型详情弹窗的编辑表单里开启/关闭代理并填写/修改地址（复用供应商的校验规则：开启必填、`http://` 开头、无认证），这样配置入口与供应商一致。
7. 作为管理员，我希望手动添加或刷新批量导入模型时不强制配置代理（默认关闭、回落供应商），这样添加流程保持简单，代理是后续一次性配置。
8. 作为管理员，我希望模型测速（详情弹窗「测试」按钮）同样按「模型→供应商→直连」解析代理，这样测试结果与真实转发的网络路径一致。
9. 作为管理员，我希望用量抓取与刷新模型列表仍只认供应商级代理，这样模型级代理不会误伤用量/刷新这两条不经转发管线的路径。
10. 作为管理员，我希望校验失败（开启代理但地址为空、地址不以 `http://` 开头、带认证信息）时返回明确的 400 错误，与供应商代理校验一致。

## Implementation Decisions

### 数据层

- **迁移 19**：`provider_model` 表新增两列（新库由 `create_table_from_entity` 直接建出；历史库在 `src/db.rs` 用 `column_exists` 逐列检测 + 单次 `ensure_migration(19, ...)` 兜底 ALTER，沿用 Migration 13/16/17 的写法与注释约定——所有缺失列 ALTER 合并进单次调用，避免版本守卫吞掉）。
  - `proxy_enabled boolean NOT NULL DEFAULT '0'`
  - `proxy_addr varchar NOT NULL DEFAULT ''`
  - 注意版本号从 **19** 起编（14/15 被生产库残留的旧 lg-proxy 方案占用）。
- `src/entity/provider_model.rs`：`Model` 增加 `proxy_enabled: bool`（`default_value = "0"`）与 `proxy_addr: String`（`default_value = ""`），与 `provider::Model` 同构。存量模型行默认关闭 → 自动回落供应商代理，无需回填。

### 转发管线（src/proxy/mod.rs）

- **Member 不带模型级代理字段**：`Member` 保持现有 `proxy_enabled`/`proxy_addr` 两个字段，语义变为「该成员最终生效的代理」。装配时解析。
- 新增私有纯函数 `resolve_proxy(model: &provider_model::Model, provider: &provider::Model) -> (bool, String)`，逻辑：
  - 模型 `proxy_enabled` 且地址非空 → 用模型地址；
  - 否则供应商 `proxy_enabled` 且地址非空 → 用供应商地址；
  - 否则 → `(false, "")`（直连）。
- `load_members` 装配 `Member` 时改调 `resolve_proxy`（原来直接取 `p.proxy_enabled/p.proxy_addr`）。
- `test_model` 构造 `Member` 时同样改调 `resolve_proxy`（函数签名已同时拿到 `provider_row` 与 `model`）。
- `forward_chat` 与 `test_model` 中既有的「`proxy_enabled && !addr.trim().is_empty()` → `Some(addr)` else `None`」换算逻辑**不变**，因为 Member 携带的已是最终生效代理。
- `upstream::call`、连接池按代理隔离的 key、CONNECT 隧道：**零改动**（上个提交已实现，见 `src/proxy/upstream.rs::call`）。
- 用量排序、`resolve_usage_map`、`fetch_and_store`：**零改动**（仍走供应商代理）。

### 路由与校验（src/routes）

- `src/routes/providers.rs`：`validate_proxy` 由私有改为 `pub(crate)`（一个词的可见性改动，不搬文件），供 provider_models 复用。
- `src/routes/provider_models.rs`：
  - `ProviderModelResponse` 增加 `proxy_enabled`/`proxy_addr`（camelCase）。
  - `UpsertProviderModelRequest`（create/update/batch 共用）增加 `#[serde(default)] proxy_enabled: bool` + `#[serde(default)] proxy_addr: String`。
  - `validate_fields` 增加 `validate_proxy(req.proxy_enabled, &req.proxy_addr, lang)` 调用。
  - create/update/batch 的 `ActiveModel` 写入两个新字段（trim 地址）。
  - `refresh_provider_models`：**零改动**（继续只读 `provider.proxy_enabled/proxy_addr`）。
  - `test_provider_model`：**零改动**（`proxy::test_model` 内部已改用 resolve_proxy）。

### 前端（web/src）

- `web/src/hooks/use-provider-models.ts`：`ProviderModel` 接口与 `ProviderModelPayload` 增加 `proxyEnabled: boolean` / `proxyAddr: string`。
- `web/src/components/provider-models/ProviderModelDetailDialog.tsx`：
  - 表单 schema 增加 `proxyEnabled`（布尔）与 `proxyAddr`（字符串），并 superRefine 校验（开启必填 + `http://` 开头）——与 ProviderEditDialog 同款。
  - 编辑态表单：共用 `ProxyConfigFields`（开关 + 条件显示地址输入，复用 `providers.proxy*` 文案与 `http://127.0.0.1:7890` placeholder），供应商弹窗 `withHint` 展示提示文案。
  - 只读态：共用 `ProviderProxyRow` 展示「网络代理」行——模型级开启 = 绿徽标 `providers.proxyOn` + `font-mono` 地址；模型级关闭但供应商级开启 = 灰徽标 `providers.proxyOff` + `providers.proxyInherited`（「继承供应商代理」，由 `providerProxyAddr` prop 传入，避免只读态误判为直连）；两层都关 = 灰徽标。
  - 打开弹窗时 `form.reset` 带入两个新字段；关闭代理时地址清空（`proxyAddr: values.proxyEnabled ? values.proxyAddr.trim() : ""`，与 ProviderEditDialog 一致）。
- 共享组件：`web/src/components/providers/ProxyConfigFields.tsx`（编辑表单代理块，供应商/供应商模型共用）、`web/src/components/providers/ProviderProxyRow.tsx`（只读代理行，供应商详情页与模型详情页共用）；`ProviderDetail` 的既有代理展示行改用 `ProviderProxyRow`（无继承层级时行为不变）。
- `web/src/i18n/locales/zh-CN.ts` / `en.ts`：新增 `providers.proxyInherited`（「· 继承供应商代理」）；其余复用 `providers.proxy*`。

### API 契约

- `POST /api/providers/{id}/models`、`POST /api/providers/{id}/models/batch`、`PUT /api/providers/{id}/models/{modelId}` 请求体增加可选字段 `proxyEnabled: boolean`（默认 false）、`proxyAddr: string`（默认 ""）。
- 所有模型列表/详情响应增加 `proxyEnabled` / `proxyAddr` 字段。
- 校验失败返回 400，错误消息与供应商代理一致（中文/英文双语，经 `Lang`）。

## Testing Decisions

三个测试 seam（用户已确认）：

1. **代理连通性测试**（`tests/provider_models_integration.rs`）：复用既有 `spawn_connect_proxy`（CONNECT 计数 mock）模式，新增用例：
   - 模型开代理（供应商也开）→ 测试请求走**模型**代理地址（断言请求到达模型代理，且模型代理地址 ≠ 供应商代理地址时仍走模型代理）。
   - 模型未开、供应商开 → 回落供应商代理（与既有 `test_refresh_models_goes_through_provider_proxy` 同构，走 `test_model` 路径）。
   - 模型与供应商都未开 → 直连（与既有 `test_refresh_models_direct_without_proxy` 同构）。
   - 注意：`spawn_connect_proxy` 只数 CONNECT 握手，无法区分不同代理地址；若需断言「走了模型代理而非供应商代理」，可让两个 mock 各自计数，或给两个 mock 不同端口、断言命中的端口。实现时选最小可行方案（两个 mock 端口各自计数）。
2. **解析优先级纯逻辑单测**（`src/proxy/mod.rs` 或 `tests/proxy_integration.rs`）：对 `resolve_proxy` 直接断言四象限——模型开/关 × 供应商开/关 → 预期 (enabled, addr)。纯函数单测，不经过网络。
3. **CRUD 与校验测试**（`tests/provider_models_integration.rs`）：
   - create 带 `proxyEnabled: true` + 合法地址 → 响应含字段；重新 GET 列表能取回。
   - update 修改代理字段 → 响应更新。
   - 开启但地址空 / 地址非 `http://` / 地址含 `@` → 400。
   - batch create 带代理字段 → 成功写入。

前端测试：`web/src/components/__tests__/provider-models-dialogs.test.tsx` 已有 ProviderModelDetailDialog 用例（编辑/测试流程），补新字段的断言（只读行展示、编辑开关+地址、开启时地址必填校验）。跑 `pnpm vitest run`。

## Out of Scope

- 用量抓取与模型刷新读取模型级代理（明确排除：这两条路径只认供应商代理，现状不变）。
- 添加模型弹窗（手动/批量导入）内直接配置代理（用户已拍板：添加后详情弹窗编辑）。
- 模型卡片上的代理角标/图标（用户已拍板：不加）。
- 代理地址认证（user:pass@）、socks 代理（维持现状：不支持）。
- 模型级代理在虚拟模型成员排序/LB 决策中的可见性（代理不影响排序，仅影响请求出口）。
- `validate_proxy` 的模块搬迁/重构（仅做可见性改动）。

## Further Notes

- 需求原文与 grilling 拍板记录见 `REQUIREMENTS.md`（同目录）。
- 交付前自查：既有代码引发的 clippy/fmt/test 必须全绿（仓库强制门禁）；`cargo fmt` 全库格式化。
- 代理地址解析失败（`reqwest::Proxy::all` 解析失败）时按无代理降级——沿用供应商代理的既有约定（`src/usage/http.rs` 注释），本次不改变该行为。
