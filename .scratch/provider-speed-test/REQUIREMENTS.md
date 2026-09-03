# REQUIREMENTS — 供应商详情：模型测速弹窗

## Scope

供应商详情页（`web/src/components/providers/ProviderDetail.tsx`，`/providers` 页右侧卡片）。

在详情左下角「更多」菜单（现有 `DropdownMenu`，含 编辑 / 删除）**新增一项「模型测速」，位于删除上方**；点击弹出对话框，列出**该供应商名下的供应商模型（ProviderModel）**，每行：左侧模型名，右侧「测试」按钮；点击测试向后端发送一次真实测试请求，**成功显示该次请求耗时**，**失败以弹窗展示错误信息**。

## Refined requirements (grilled)

- **耗时口径**：后端返回（用户拍板）。扩展现有 `/api/providers/{id}/models/{model_id}/test`（`proxy::test_model` 内部已测得 start/end/connect 计时但未返回），让响应携带该次请求耗时；前端用后端返回值展示。与 request 表指标同口径，排除本机→服务器网络。
- **测试记录**：沿用现状落库（用户拍板）。每次测速与现有「测试」按钮一致，经 `test_model` 落一条 request 记录（`TEST_VIRTUAL_MODEL_ID`/`TEST_API_KEY_NAME` 标记），可溯源，不计特殊处理。
- **停用供应商**：仍可测速（用户拍板）。测试直连上游、绕过 enable 检查（现 `/test` 接口即如此），正用于验证能否恢复。
- **菜单位置**：现有「更多」菜单（左下角）内，`编辑` 与 `删除` 之间新增「模型测速」项（删除上方）。
- **列表来源**：全量 `useProviderModels()` 查询结果按 `m.providerId === provider.id` 客户端过滤（复用 `/provider-models` 页既有写法）。
- **每行动作**：左侧模型名（`provider_model_id`，非目录通用名，与 `/provider-models` 页展示一致），右侧「测试」按钮。单模型独立测试，非批量。
- **成功态**：行内显示该次请求耗时（后端返回，单位 ms，前端格式化）。
- **失败态**：后端 `test_model` 返回人类可读错误（连接/超时/上游状态+错误体），以弹窗（Dialog/Alert 形式）展示。
- **打开弹窗时机**：点菜单「模型测速」打开；弹窗内列表按当前选中供应商加载。
- **停用态按钮**：不因 enable=false 禁用测试（见上）。

## Non-goals (ponytail cuts)

- **不新增后端端点**：复用 `POST /api/providers/{provider_id}/models/{model_id}/test`；只在其响应中增加耗时字段。现有前端 `useTestProviderModel` hook 同步扩展返回类型。
- **不做批量/全测速**：无「全部测试」按钮，无并发测速；一行一测，用户点谁测谁。将来要加再扩展。
- **不做测速历史**：不存每次测速的耗时记录表；仅沿用每次落一条 request 记录的现状，不做耗时聚合。
- **不新增领域词条/ADR**：「测速」复用既有领域动作「测试 (Test)」语义（发最小化请求验证上游连通与耗时），无新概念；菜单文案用「模型测速」贴近用户原话。
- **不新增 i18n 基础设施**：沿用现有 `web/src/i18n` 资源文件（`zh-CN.ts`/`en.ts`），仅补该功能所需 key。
- **不抽取通用弹窗原语**：仅此一处使用；直接以 shadcn `Dialog` 组件在 `components/providers/` 内实现。
- **不新增 npm/Rust 依赖**：图标用已装的 lucide-react（如 `Gauge`/`Activity`）。

## Open questions resolved by grilling

- 耗时口径：后端返回（非前端浏览器计时）。
- 是否计入统计：沿用现状落库（每次测速一条 request 记录）。
- 停用供应商能否测速：可以（绕过 enable，正用于验证能否恢复）。
- 弹窗与菜单位置、成功/失败展示形态：见上（左菜单项右上、列表行内耗时、失败弹窗）。

## Reference (code facts, explored)

- 后端测试链路：`src/routes/provider_models.rs:606-655` `test_provider_model` → `src/proxy/mod.rs:1646-1781` `test_model`（直连上游，已算 `start_time`/`end_time`/`output_tokens_time` 只落库不返回）；`build_upstream_call` 四协议转换 `src/proxy/mod.rs:458-538`。
- 前端数据源：`web/src/hooks/use-provider-models.ts` `useProviderModels()`（全量，`ProviderModel{providerId,providerModelId,...}`）、`useTestProviderModel(providerId)`（`POST .../models/{modelId}/test`，60s 超时）。
- 详情页菜单：`web/src/components/providers/ProviderDetail.tsx:262-286`（`DropdownMenu`，编辑 / 分隔 / 删除，`onEdit`/`onDelete` 由父级 `providers.tsx` 注入 state）。
- 失败错误结构：HTTP 502/400 + `{code,msg}`（`src/response.rs`），前端 `ApiError` 携带 `msg`。
