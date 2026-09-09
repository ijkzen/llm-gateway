# 17 · FE 配置管理域审查

Type: task
Status: open
Blocked by: 01

## Question

对前端配置管理域做全量审查：`pages/providers.tsx` / `provider-models.tsx` / `virtual-models.tsx` / `provider-overview.tsx` / `virtual-model-overview.tsx` / `api-keys.tsx` + `components/providers/` / `provider-models/`（AddProviderModelsDialog 824 行等）/ `virtual-models/`（VirtualModelEditDialog 685 行等）及 hooks 与 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：编辑弹窗表单校验（zod/受控态）、镜像/继承字段门控模板、级联刷新时机、列表缓存失效（query invalidation）面；
- 实现简洁：三个大弹窗的重复表单项/校验模式、detail 弹窗与页面跳转双形态并存面；
- 测试覆盖：组件级用例之外缺什么（弹窗提交流程、字段门控矩阵）；
- 模块间调用：与共享 data-table/ui 层的契约、与后端 CRUD 字段名同步面。

产出 `.scratch/code-quality-map-2026-09-09/findings/17-fe-configuration-management.md`，Answer 给摘要与需拍板问题。
