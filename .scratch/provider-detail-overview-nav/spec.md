# 供应商详情卡标题跳转数据面板 — Spec

> Feature slug: `provider-detail-overview-nav`。由 `REQUIREMENTS.md`（grill-with-docs + ponytail 产出）合成，
> 按项目 issue tracker 约定存放于 `.scratch/provider-detail-overview-nav/`。
> 参考规格：`.scratch/provider-models/spec.md`（含已合入的 overview 导航入口 6405384 / dac8057）。

## Problem Statement

供应商管理页（`/providers`）右侧的详情卡（`ProviderDetail`）是查看单个供应商配置的入口，
但卡标题目前只是纯文本的供应商名。供应商模型页的供应商分组标题、模型详情弹窗标题早已支持
「点名称 + 方向键进入该对象的数据面板」；供应商详情卡作为供应商维度的主界面，标题却没有
直达 `/providers/{id}/overview`（供应商数据面板）的入口，管理员要看某个供应商的调用/Token/赛马
指标时得先记住 id、改 URL 或绕道其它页面。

## Solution

把供应商管理页右侧详情卡标题（`CardHeader` 内 `CardTitle` 的供应商名文本）变为**可点击链接**，
点击整页导航到该供应商的数据面板 `/providers/{id}/overview`。视觉与交互对齐仓库既有导航模式
（`ProviderModelSection` 供应商分组标题、`ProviderModelDetailDialog` 弹窗标题，及 `f6c6b06`
收窄后的样式）：标题文字 + ChevronRight 方向图标，hover 高亮只覆盖这两者。

## User Stories

1. 作为网关管理员，我想在供应商管理页右侧详情卡点击供应商名（带方向键）直接进入该供应商的数据面板，
   以便查看其调用量、Token 用量与模型赛马，而无需手动改 URL。
2. 作为网关管理员，我希望详情卡标题变成链接后，长供应商名仍被中间省略截断展示，以便超长名不撑破卡片。
3. 作为网关管理员，我希望鼠标悬停标题时能看到「查看 X 的数据面板」提示与高亮反馈，以便明确该处可点击。
4. 作为网关管理员，我希望该链接不干扰详情卡其余部分：点标题才跳转，点 baseUrl 副行、启停开关、
   底部更多菜单等仍走各自原有行为。

## Implementation Decisions

- 改动仅限前端单一组件：`ProviderDetail`（供应商管理页右侧详情卡）。`CardTitle` 内的供应商名文本
  改为 `Link`（react-router），目标 `/providers/{provider.id}/overview`。
- 样式对齐 `f6c6b06` 收窄版导航模式：
  - 供应商名外包 `MidEllipsis`（长名自适应中间省略，符合仓库超长文本规范，禁止尾部 truncate）。
  - Link 结构：`inline-flex max-w-full min-w-0 items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60`。
  - 方向图标 ChevronRight：`size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground`。
  - hover 高亮与方向微移只作用于标题文字 + 图标（不整卡可点）。
- `title` tooltip 复用已存在但未使用的 i18n key `providerModels.viewProviderOverview`
  （zh-CN「查看 {{provider}} 的数据面板」/ en「View {{provider}} overview」，带 `{ provider }` 插值），不新增 key。
- 不新增后端接口 / 不改路由表（`/providers/:providerId/overview` 路由与页面已存在）/ 不改列表 `ProviderList`。
- 若详情卡标题区布局（左标题右启停开关）因此需要微调对齐，保持标题与开关的原相对位置。

## Testing Decisions

- 只测外部行为：渲染 `ProviderDetail` 后标题区存在指向 `/providers/7/overview` 的链接（`href` 断言），
  链接内可见供应商名与方向图标，并带 `查看 X 的数据面板` tooltip。不 mock 内部实现、不新增 seam。
- 落点：既有组件测试文件 `web/src/components/__tests__/provider-detail.test.tsx` 补一个断言用例
  （该文件已 mock `use-providers`/`use-toast` 等依赖，直接沿用其 render 辅助）。
- 参照先例：同文件既有开关/菜单/明文展示断言；`provider-models` 相关弹窗/分组的导航断言。

## Out of Scope

- 供应商模型页 / 虚拟模型页 / API Key 页的导航入口（均已存在或另有工单）。
- 详情卡内其它字段（baseUrl、API Key 行、折叠区等）不加导航。
- 不做「新窗口打开」、不做 hover 高亮组件化复用、不抽共享可点标题组件。
- 不改后端与路由。

## Further Notes

- 语义对齐：仓库已有「名称 + ChevronRight → 数据面板」的事实标准（分组标题、模型弹窗、API Key name 列），
  本改动是同一模式的又一落点，不引入新交互范式。
