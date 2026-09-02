# SPEC — 请求日志上游模型下拉按供应商分组

Label: `ready-for-agent`

## Problem Statement

请求日志页的「上游模型」过滤下拉只显示模型 ID 字符串。当用户不选择供应商时，不同供应商提供的同名模型在下拉中重复出现，用户无法分辨每个选项属于哪个供应商；选中后也不知道日志会按哪个供应商的模型匹配。

## Solution

上游模型下拉改为按供应商分组展示：每个供应商一个分组（供应商名作为分组标题），组内列出该供应商的模型。未选供应商时所有供应商的分组依次展示；选中某供应商后仅剩该供应商一组（现有级联过滤行为不变）。

## User Stories

1. 作为管理员，我想在上游模型下拉中按供应商分组看到模型，以便不选供应商时也能一眼分辨同名模型属于哪个供应商。
2. 作为管理员，我想让分组顺序与供应商列表顺序一致，以便按我熟悉的供应商优先级浏览模型。
3. 作为管理员，我在选中某供应商后打开模型下拉，只看到该供应商一组，以便快速定位该供应商的模型。
4. 作为管理员，我在未选供应商时选中某模型名，日志仍跨供应商匹配所有同名模型，以便一次查看该模型在所有供应商上的表现。
5. 作为管理员，遇到供应商列表中查不到的模型条目时，分组标题兜底显示供应商 ID，以便不出现空白分组。

## Implementation Decisions

- 仅改前端请求日志过滤卡片中「上游模型」Select 的内容渲染：用 shadcn/ui 既有 `SelectGroup` + `SelectLabel` 按供应商分组，不再平铺 `SelectItem`。
- 分组数据由现有 `providerModelOptions`（已按所选供应商级联过滤）与供应商列表在渲染处归并：按供应商列表顺序分组，组内保持 `providerModelOptions` 原有顺序。
- 供应商名从供应商列表按 `providerId` 查找，查不到时分组标题显示 `#<providerId>` 兜底。
- 「全部」选项保留在所有分组之前。
- 已选供应商时选项只剩一组，走同一渲染逻辑，不做特判。
- 后端、API 契约、hooks、类型定义零改动；选中同名模型仍是 `model_id` 精确匹配跨供应商行为。
- 供应商/模型过滤项及联动（选供应商清空模型、级联过滤）保持现状不动。

## Testing Decisions

- 只测外部行为：打开模型下拉后分组标题（供应商名）与组内模型选项的对应关系、同名模型分属各自供应商组；不测内部 memo/归并实现。
- 测试落点：复用既有 `request-logs.test.tsx` 组件测试（该文件已 mock `useProviders`/`useProviderModels`，零新接缝）。
- 先例：同文件内「重置按钮清空过滤」用例已演示打开 Select → 点击 option 的交互方式；新增用例沿用 `fireEvent.click` + `getAllByRole("option")` 模式。

## Out of Scope

- 换成可搜索 Combobox（用户已拍板用分组）。
- 后端 `model_id` 过滤语义变更（不隐式锁定供应商）。
- 其他过滤项、其他页面、表格列（供应商列）改动。

## Further Notes

- 用户原始需求中「过滤项 + 联动」部分经代码调查确认已在 main 实现，用户确认本期不做。
- worktree：`feat/request-log-provider-model-filter`（`/Users/ijkzen/Projects/RUST-Project/llm-gateway-reqlog-filter`）。
