# 供应商详情卡标题跳转数据面板 — Requirements

> 本文件由 start_work 流程产出：Stage 1 grill-with-docs（用户拍板）+ ponytail 收敛。

## 来源

用户原话（2026-09-07）：「在供应商详情页的供应商标题添加导航到数据面板的能力，规格参考供应商模型页面」。

参考规格：供应商模型页面既有导航入口——`ProviderModelSection` 供应商分组标题（`ProviderModelSection.tsx:53-62`）、
`ProviderModelDetailDialog` 弹窗标题（`ProviderModelDetailDialog.tsx:193-202`），以及最近收窄 hover 高亮的
`f6c6b06`（模型弹窗标题栏 hover 高亮只覆盖模型 ID 与方向图标）。

## 目标（Scope）

供应商管理页（`/providers`，侧边栏「供应商」）为左列表 + 右侧详情卡（`ProviderDetail`）布局。
把右侧详情卡标题（`CardHeader` 内 `CardTitle`，现为纯文本 `provider.name`）改为**可点击链接**，
点击整页导航到该供应商的数据面板 `/providers/{id}/overview`。

- 改的是**供应商管理页右侧详情卡的标题**（用户拍板），不是模型弹窗/分组标题——那些已有导航。
- 样式对齐仓库既有导航模式（与 `ProviderModelSection`/`ProviderModelDetailDialog`/`f6c6b06` 收窄版一致）：
  - 供应商名文字包 `MidEllipsis`（长名截断），`className="group inline-flex max-w-full min-w-0 items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60"`。
  - 文字右侧 `ChevronRight` 方向图标：`className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground"`（hover 右移微动 + 变亮）。
  - hover 高亮只覆盖「标题文字 + 方向图标」（用户拍板），不整卡可点。
- `title` tooltip 复用**已存在但未使用**的 i18n key `providerModels.viewProviderOverview`（`zh-CN: "查看 {{provider}} 的数据面板"` / `en: "View {{provider}} overview"`），不新增 key。
- 链接目标 `/providers/${provider.id}/overview`（数据面板是独立路由页，整页导航，用户拍板）。

### 非目标（ponytail 收敛）

- 不抽共享「可点标题链接」组件（仓库各入口已各写一份，复用样式即可，不新增抽象）。
- 不改后端、不改路由表（`/providers/:providerId/overview` 已存在）、不改列表页 `ProviderList`、不改其它页面。
- 不做 hover 高亮组件化、不做「新窗口打开」。

## 测试

在既有 `web/src/components/__tests__/provider-detail.test.tsx` 补断言：渲染后标题区存在指向
`/providers/7/overview` 的链接（供应商名可点），带方向图标与 tooltip。跑 `pnpm vitest run` 验证。

## 开放问题（grilling 已全部拍板）

1. 落点 = 供应商管理页右侧详情卡标题（非模型弹窗描述里的供应商名）。
2. hover 高亮范围 = 标题文字 + 方向图标（f6c6b06 收窄版）。
3. 导航形态 = 标题文字变整页链接（数据面板独立路由，整页跳转）。
