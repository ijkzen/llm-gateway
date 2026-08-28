# 前端管理后台完整改版设计文档

## 1. 背景与目标

当前 `rs-template-web` 前端页面基于 shadcn/ui 默认组件搭建，视觉风格偏向"开箱即用"的灰阶模板，缺乏品牌感和专业管理后台的质感。本次改版目标：

- 建立统一的 Indigo/Slate 设计系统，摆脱默认模板感。
- 在不改动后端 API 的前提下，提升信息密度、可读性与操作效率。
- 优化布局、页面结构、表格展示、空状态与加载状态。
- 新增 Overview 总览页，让入口更有信息量。
- 保证 Light / Dark 双主题一致且精致。

## 2. 设计系统

### 2.1 色彩

| Token | Light 模式 | Dark 模式 | 说明 |
|---|---|---|---|
| `--background` | Slate 50 (`hsl(210 40% 98%)`) | Slate 950 (`hsl(222.2 84% 4.9%)`) | 页面背景 |
| `--foreground` | Slate 900 | Slate 50 | 主文字 |
| `--card` | White | Slate 900 | 卡片背景 |
| `--card-foreground` | Slate 900 | Slate 50 | 卡片文字 |
| `--primary` | Indigo 600 (`#4f46e5`) | Indigo 500 (`#6366f1`) | 主色 |
| `--primary-foreground` | White | White | 主色上的文字 |
| `--secondary` | Slate 100 | Slate 800 | 次级背景 |
| `--secondary-foreground` | Slate 900 | Slate 50 | 次级文字 |
| `--muted` | Slate 100 | Slate 800 | 弱背景 |
| `--muted-foreground` | Slate 500 | Slate 400 | 弱文字 |
| `--accent` | Indigo 50 | Indigo 950 | 强调背景 |
| `--accent-foreground` | Indigo 900 | Indigo 50 | 强调文字 |
| `--destructive` | Red 600 | Red 500 | 危险/删除 |
| `--destructive-foreground` | White | White | 危险上的文字 |
| `--border` | Slate 200 | Slate 800 | 边框 |
| `--radius` | `0.75rem` (`12px`) | 同上 | 圆角 |
| `--success` | Emerald 600 | Emerald 500 | 成功/启用（语义色，用于 StatusBadge 等） |
| `--warning` | Amber 500 | Amber 500 | 警告（语义色，用于 StatusBadge 等） |

### 2.2 字体与排版

- 页面标题：`text-3xl font-bold tracking-tight`
- 页面描述：`text-base text-muted-foreground`
- 卡片标题：`text-lg font-semibold`
- 表格表头：`text-xs font-medium uppercase tracking-wider text-muted-foreground`
- 代码/表达式：`font-mono text-sm`

### 2.3 间距与尺寸

- 页面内边距：`p-6`（桌面端），`p-4`（移动端）
- 卡片内边距：`p-5` 或 `p-6`
- 卡片阴影：`shadow-sm`
- 卡片圆角：`rounded-xl`
- 组件间距：`gap-4` 或 `gap-6`

### 2.4 组件质感

- 所有卡片使用 `bg-card shadow-sm rounded-xl border`。
- 表格行使用 `hover:bg-muted/50`。
- 按钮统一使用 `transition-colors duration-200`。
- 可交互元素保持清晰的 `focus-visible:ring-2 focus-visible:ring-primary`。

## 3. 布局重构

### 3.1 Sidebar

- 品牌区：左侧 indigo 圆角 logo（`size-10 rounded-xl bg-primary text-primary-foreground`）+ 「RS Template」+ 「管理后台」。
- 导航项：hover 时左侧出现 3px indigo 竖条，背景变为 `bg-sidebar-accent`。
- 当前激活项：背景 `bg-sidebar-accent`，文字 `text-sidebar-accent-foreground`，左侧竖条高亮。
- 底部：显示当前环境（`dev` / `prod`）和版本号 `v0.1.0`。
- 保持 collapsible 能力（shadcn/ui Sidebar 已内置）。

### 3.2 Header

- 左侧：当前页面标题或简单面包屑（因导航层级较浅，可显示当前页面名称，如「定时任务」）。
- 中间：全局搜索框，本阶段仅做客户端本地过滤（按当前页数据过滤），后续可扩展为服务端搜索。
- 右侧：
  - 刷新数据按钮（触发当前页面数据重取）
  - 主题切换按钮
  - 设置入口图标（链接到 `/settings`）

### 3.3 内容区

- 使用 `SidebarInset` 内部容器限制最大宽度，避免超宽屏幕下表格过度拉伸。
- 统一结构：`PageHeader` + 内容区（Stats Cards / 工具栏 / 数据卡片）。

## 4. 新增页面：Overview（总览）

将当前 `/` 的默认跳转行为改为渲染 Overview 页面。

### 4.1 顶部 Stats Cards

横向 4 列卡片：

1. **定时任务总数**：`Clock` 图标 + 数字 + 「全部任务」
2. **已启用任务数**：`Play` 图标 + 数字 + 「运行中」
3. **系统设置项数**：`Settings` 图标 + 数字 + 「配置项」
4. **最近执行时间**：`Activity` 图标 + 相对时间 + 「最近一次 Cron 执行」。若尚无任务执行过，显示「—」或「等待执行」。

> 数据由前端聚合现有 `/api/cron-jobs` 与 `/api/settings` 返回结果计算，不依赖新接口。

### 4.2 下方两栏

- **左栏：最近执行的 Cron 任务**
  - 取 `last_run_at` 有效且最新的 5 条任务。
  - 展示：任务名、分组、上次执行时间（相对时间）。
  - 点击跳转到 `/cron-jobs` 并自动滚动定位（可选）。

- **右栏：最近更新的 Setting**
  - 取 `updated_at` 最新的 5 条设置。
  - 展示：键、值（截断）、更新时间。
  - 点击直接打开编辑 Dialog。

### 4.3 空状态

如果没有任何数据，两栏均显示 `EmptyState` 组件。

## 5. Cron Jobs 页面重构

### 5.1 PageHeader

- 左侧：`Clock` 图标 + 标题「定时任务」+ 描述「管理系统定时任务，支持启用/禁用、立即执行与编辑」。
- 右侧：「刷新」按钮。

### 5.2 Stats Cards 行

横向 4 列：

1. 总任务数
2. 已启用任务数
3. 分组数量
4. 最近执行时间

### 5.3 工具栏

- 左侧：搜索框（按 `name` / `title` / `expression` 过滤）。
- 中间：分组筛选 Dropdown（全部 + 实际存在的分组）。
- 右侧：状态筛选 Switch（全部 / 仅启用 / 仅禁用）。

### 5.4 表格改进

- 外层使用卡片容器（`rounded-xl border bg-card shadow-sm`）。
- 表头背景使用 `bg-indigo-50/50 dark:bg-indigo-950/30`，文字为 muted-foreground。
- 行 hover：`hover:bg-muted/50`。
- 列内容：
  - **名称**：`font-medium`
  - **标题**：普通文字
  - **表达式**：`font-mono` + `Badge variant="outline"`
  - **状态**：Switch + `StatusBadge`（启用=success，禁用=secondary）
  - **上次执行 / 下次执行**：`RelativeTime` 组件
  - **操作**：`DropdownMenu`（⋮），选项为「立即执行」「编辑」「删除」
- 分组标题：更突出的 header，带任务数量 badge。
- 空状态：使用 `EmptyState`。

### 5.5 Dialog 统一

- `CronJobEditDialog` 与 `CronJobDeleteDialog` 保持现有逻辑，仅调整标题、按钮、间距以匹配新设计系统。

## 6. Settings 页面重构

### 6.1 PageHeader

- 左侧：`Settings` 图标 + 标题「系统设置」+ 描述「管理应用配置项」。
- 右侧：搜索框。

### 6.2 工具栏

- 搜索框（按 `key` / `value` 过滤）。
- 类型筛选 Dropdown（All / String / Int / Float / Bool）。

### 6.3 表格改进

- 卡片容器 + 阴影。
- 列内容：
  - **键**：`font-medium`
  - **值**：`max-w-md truncate`，hover tooltip 显示完整值
  - **类型**：彩色 `Badge`：String=indigo, Int=blue, Float=amber, Bool=emerald
  - **更新时间**：`RelativeTime`
  - **操作**：文字按钮「编辑」或 icon 按钮
- 空状态：`EmptyState`。

### 6.4 Dialog 统一

- `SettingEditDialog` 样式与新设计系统一致。

## 7. 新增共享组件

### 7.1 PageHeader

```tsx
interface PageHeaderProps {
  icon?: LucideIcon;
  title: string;
  description?: string;
  children?: React.ReactNode; // 右侧操作区
}
```

### 7.2 StatsCard

```tsx
interface StatsCardProps {
  icon: LucideIcon;
  label: string;
  value: string | number;
  subLabel?: string;
  trend?: "up" | "down" | "neutral";
}
```

### 7.3 EmptyState

```tsx
interface EmptyStateProps {
  icon?: LucideIcon;
  title: string;
  description?: string;
  action?: React.ReactNode;
}
```

### 7.4 StatusBadge

```tsx
interface StatusBadgeProps {
  status: "enabled" | "disabled" | "success" | "error" | "warning";
  label?: string;
}
```

### 7.5 RelativeTime

```tsx
interface RelativeTimeProps {
  date: string | Date;
  fallback?: string;
}
```

显示相对时间（如「3 分钟前」「昨天」），hover 时通过 Tooltip 显示完整时间。

### 7.6 SearchInput

带搜索图标的输入框，支持 `value` / `onChange` / `placeholder`。

### 7.7 DataTableToolbar

表格工具栏容器，统一搜索、筛选、操作的布局与间距。

## 8. 交互与状态

### 8.1 加载状态

- 不再使用整页骨架，而是按区块加载：
  - `PageHeaderSkeleton`
  - `StatsCardsSkeleton`（4 个卡片骨架）
  - `TableSkeleton`（表头 + 5 行骨架）
- 每个区块独立显示 loading，减少页面跳动感。

### 8.2 空状态

- 所有列表/表格空数据时使用统一 `EmptyState` 组件。
- 包含图标、标题、描述，以及可选操作按钮。

### 8.3 Toast 提示

- 成功：标题前加 `CheckCircle` icon，使用默认样式。
- 失败：标题前加 `XCircle` icon，使用 `destructive` variant。
- 信息：标题前加 `Info` icon，使用默认样式。

### 8.4 微交互

- 所有按钮、链接、卡片 hover 使用 `transition-colors duration-200`。
- 主题切换保留现有 View Transition 动画。
- 表格操作 DropdownMenu 使用 `side="left"` 避免超出视口。

### 8.5 键盘与可访问性

- 所有可交互元素保持 `focus-visible` 焦点环。
- 操作按钮保留 `aria-label`。
- Switch 保留 `aria-label`。

## 9. 路由调整

`web/src/App.tsx` 更新为：

```tsx
<Route element={<AppLayout />}>
  <Route path="/" element={<OverviewPage />} />
  <Route path="/cron-jobs" element={<CronJobsPage />} />
  <Route path="/settings" element={<SettingsPage />} />
</Route>
```

移除 `/` 到 `/cron-jobs` 的默认跳转。

## 10. 文件改动清单

### 新增文件

- `web/src/pages/overview.tsx`
- `web/src/components/page-header.tsx`
- `web/src/components/stats-card.tsx`
- `web/src/components/empty-state.tsx`
- `web/src/components/status-badge.tsx`
- `web/src/components/relative-time.tsx`
- `web/src/components/search-input.tsx`
- `web/src/components/data-table-toolbar.tsx`
- `web/src/components/page-header-skeleton.tsx`
- `web/src/components/stats-cards-skeleton.tsx`

### 修改文件

- `web/src/index.css`：颜色变量、dark 模式。
- `web/src/App.tsx`：新增 overview 路由。
- `web/src/components/layout.tsx`：Sidebar、Header 重构。
- `web/src/components/theme-toggle.tsx`：保持功能，可选样式微调。
- `web/src/pages/cron-jobs.tsx`：接入 PageHeader、StatsCards、工具栏。
- `web/src/pages/settings.tsx`：接入 PageHeader、工具栏。
- `web/src/components/cron-jobs/CronJobTable.tsx`：表格卡片化、DropdownMenu、状态标签、相对时间。
- `web/src/components/cron-jobs/CronJobEditDialog.tsx`：样式统一。
- `web/src/components/cron-jobs/CronJobDeleteDialog.tsx`：样式统一。
- `web/src/components/settings/SettingsTable.tsx`：表格卡片化、彩色 badge、相对时间。
- `web/src/components/settings/SettingEditDialog.tsx`：样式统一。
- `web/src/components/page-skeleton.tsx`：拆分为区块骨架或调整结构。

## 11. 建议实现顺序

为降低风险并便于逐步验收，建议按以下顺序实现：

1. **设计系统奠基**：修改 `index.css` 颜色变量，调整 `layout.tsx` 的 Sidebar 与 Header 基础样式。
2. **共享组件**：实现 `PageHeader`、`StatsCard`、`EmptyState`、`StatusBadge`、`RelativeTime`、`SearchInput`。
3. **Overview 页面**：新增总览页并更新路由，验证 StatsCard 与数据聚合逻辑。
4. **Cron Jobs 页面**：接入 PageHeader、Stats Cards、工具栏、新表格样式与 DropdownMenu 操作列。
5. **Settings 页面**：接入 PageHeader、工具栏、新表格样式、彩色类型 Badge 与相对时间。
6. ** polish**：统一 Dialog 样式、优化 loading skeleton、跑 lint/format、做 Light/Dark/移动端验收。

## 12. 约束与边界

- **不改动后端 API**：所有数据仍来自现有 `/api/cron-jobs`、`/api/settings` 及相关操作接口。
- **不新增后端字段**：Overview 的统计数据由前端聚合计算。
- **保持功能不变**：现有启用/禁用、立即执行、编辑、删除逻辑保持不变。
- **保持响应式**：移动端 sidebar 可收起，表格支持横向滚动。
- **保持双主题**：所有新增组件必须同时适配 light / dark 模式。
- **不引入新依赖**：尽量使用已安装的 shadcn/ui、lucide-react、tailwind-merge 等库。

## 13. 验收标准

- [ ] Overview 页面正常显示，统计数字与最近列表正确。
- [ ] Cron Jobs 页面使用新布局、新表格、DropdownMenu 操作列。
- [ ] Settings 页面使用新布局、彩色类型 badge、相对时间。
- [ ] Light / Dark 主题切换后无明显视觉问题。
- [ ] 移动端布局可正常浏览（sidebar 收起、表格滚动）。
- [ ] 所有现有功能（启用/禁用、立即执行、编辑、删除）正常工作。
- [ ] 空状态与加载状态使用新组件。
- [ ] `pnpm lint` 与 `pnpm format` 通过。
