# Spec: 供应商详情 —— 模型测速弹窗

Status: ready-for-agent

## Problem Statement

管理员在供应商详情页需要确认名下各模型的实际连通与响应速度（常用于停用后验证能否恢复、或挑选快模型）。当前只有进入「供应商模型」二级页、逐模型打开详情、点「测试」才能发一次测试请求——且成功只提示「模型测试成功」，看不到耗时。详情页本身没有模型维度的测速入口。

## Solution

在供应商详情卡片左下角「更多」菜单（现有编辑 / 删除上方）新增「模型测速」项。点击弹出对话框，列出该供应商名下全部供应商模型（ProviderModel），每行左侧为模型名（`provider_model_id`）、右侧为「测试」按钮；点击测试即向后端现有测试接口发送真实最小化请求，成功在行内显示本次请求耗时（ms），失败以弹窗展示后端返回的错误信息。

## User Stories

1. 作为管理员，我在供应商详情左下角「更多」菜单中看到新增的「模型测速」项，它位于「删除」上方，以便在不离开详情页时发起测速。
2. 作为管理员，我点击「模型测速」后弹出对话框，其中列出当前选中供应商名下的全部模型，每行左侧是模型名，以便对单个模型逐一测速。
3. 作为管理员，我点击某行的「测试」按钮后，系统向后端现有测试接口发送一次真实最小化请求（与模型详情里的「测试」同方案），以便验证该模型上游真实可用性。
4. 作为管理员，测速成功后，该行内显示本次请求耗时（后端口径，ms），以便比较各模型响应快慢。
5. 作为管理员，测速失败时，以弹窗形式展示后端返回的错误信息（连接失败/超时/上游 HTTP 错误等人类可读原因），以便定位问题。
6. 作为管理员，某行测速进行中，该行测试按钮呈加载态并防重复点击，以便不会并发打爆上游。
7. 作为管理员，供应商下没有任何模型时，弹窗内显示空态提示而非空白列表，以便理解原因。
8. 作为管理员，即使该供应商处于停用状态（enable=false），我仍可对模型发起测速，以便验证上游是否已恢复、决定是否重新启用。
9. 作为管理员，每次测速仍写入 request 表（与现有测试按钮一致，可溯源），以便请求日志可查。
10. 作为管理员，关闭弹窗后再次打开，之前的耗时/错误状态被清除、恢复初始态，以便不残留上一次结果。

## Implementation Decisions

### 改动范围

- **后端**：复用现有 `POST /api/providers/{provider_id}/models/{model_id}/test`，不新增端点。仅将其响应从 `{ ok: true }` 扩展为携带耗时字段；`proxy::test_model` 由 `Result<(), String>` 改为返回耗时信息（沿用其已计算并落库的 `output_tokens_time = end_time - reply.start_at_ms` 口径——即上游从响应开始到读完的耗时，TTFT 后的处理+传输时间）。失败路径维持现状（HTTP 502 + `{code,msg}` 人类可读原因）。
- **前端数据层**：`useTestProviderModel(providerId)` 的 mutation 返回值由 `Promise<void>` 改为 `Promise<number>`（耗时 ms，从响应 data 解出）。现有唯一调用方（模型详情弹窗测试按钮）仅将结果用于成功提示，忽略该返回值即可，无需改动其行为。
- **前端 UI**：
  - `ProviderDetail` 新增「更多」菜单项「模型测速」（位于现有分隔线与「删除」上方），通过新的 `onSpeedTest(provider)` prop 上抛。
  - 父级 `/providers` 页面新增 `speedTestProvider` state，挂载新弹窗 `<ProviderSpeedTestDialog>`（模式同 `ProviderEditDialog`/`ProviderDeleteDialog`：由父级持 state 与 open）。
  - 新弹窗组件接收 `provider`，用现有全量 `useProviderModels()` 数据源按 `providerId === provider.id` 客户端过滤（同 `/provider-models` 页写法）得到该供应商模型列表；逐行测试用现有 `useTestProviderModel`。
- **组件形态**：shadcn `Dialog`（标题「模型测速」+ 所属供应商名副标题），内容为每模型一行：左侧 `provider_model_id`（截断、`font-mono`），右侧「测试」按钮（测速中该行显示 spinner + 禁用）。行内成功态显示耗时（`~123 ms` 或后端口径格式化）；错误以 `ConfirmDialog` 弹窗展示（复用现有 `ProviderModelDetailDialog` 的测试失败弹窗范式：标题 + 错误详情 `<pre>`）。
- **图标/依赖**：菜单项图标与按钮加载态用已装 lucide-react（如 `Gauge`/`Activity`/`FlaskConical`/`Loader2`）；不新增任何 npm/Rust 依赖。
- **i18n**：沿用现有 `providerModels.*` 命名空间补充测速所需文案（中英双语），不新建立方。

### 待实现的精确决策

- **耗时返回结构**：后端成功响应 `data` 形如 `{ ok: true, duration_ms: number }`；`test_model` 成功分支返回 `duration_ms`，handler 一并放入 `data`。失败分支维持 `scheduler_error(BAD_GATEWAY, message)`，前端 `ApiError.message` 即展示内容。
- **成功态展示文案**：用户明确要「显示请求耗时」。行内成功显示耗时值，可带短前缀（如「耗时」+ 数值 + ms）。
- **失败弹窗**：`ConfirmDialog` 单确认按钮「关闭」，内容为后端错误 message（`font-mono`、可换行、destructive 色），复用现有 `providerModels.testFailedTitle`/`testFailedDesc` 语义（若复用的 key 措辞与场景契合则直接复用，否则新增测速专属 key）。
- **空态**：无模型时展示「该供应商暂无模型」提示，引导去「供应商模型」页添加（不强行内嵌导航）。

## Testing Decisions

### 接缝（用户已拍板两层）

- **前端弹窗行为测试**（主接缝）：扩展现有 `web/src/components/__tests__/provider-detail.test.tsx`（已有 ProviderDetail 渲染 + mock 基建），新增对测速弹窗的用例：
  1. 点「更多」→ 菜单含「模型测速」（位于删除上方）。
  2. 点「模型测速」→ 弹窗打开，列出该供应商模型（mock `useProviderModels` 返回多条、含当前 provider 与其他 provider，断言只显示本供应商的）。
  3. 点某行「测试」→ mock `useTestProviderModel` 成功返回耗时 → 行内出现该耗时。
  4. mock `useTestProviderModel` 失败 → 出现失败弹窗含错误 message；点「关闭」后消失。
  5. 无模型供应商 → 弹窗显示空态。
  6. 关闭重开 → 状态复位。
  - 范式：仅断言外部可见行为（role/text），不断言内部 state；mock 数据 hooks 与既有测试一致。
- **后端耗时返回测试**：扩展现有 `tests/provider_models_test_integration.rs`（已有 mock 上游 + `/test` 成功/失败用例与 `call_test` helper），在成功用例断言响应 `data` 含 `duration_ms` 数值字段。失败用例断言维持现状（502 + 错误 message）。

## Out of Scope

- 不做批量测速 /「全部测试」/并发测速。
- 不做耗时历史记录 / 多次测速聚合。
- 不改现有「供应商模型」二级页与单模型详情弹窗的测试行为（仅其 hook 返回值类型扩展、忽略即可）。
- 不改变 request 表统计口径（测速照常计入）。
- 不做耗时分布图 / 阈值告警。

## Further Notes

- 领域词表（CONTEXT.md）中「供应商模型 (ProviderModel)」是既有概念；「测速」复用既有「测试」语义，不新增术语。
- 后端 `test_model` 无现成单测；`/test` 的行为已由 `provider_models_test_integration.rs` 覆盖，扩展该处为最省接缝。
- 停用供应商仍可测速：现 `/test` 接口不检查 enable，弹窗按钮亦不因 enable=false 禁用（沿用既有语义）。
