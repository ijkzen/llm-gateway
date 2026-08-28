# 前端管理后台完整改版 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `rs-template-web` 从默认 shadcn 灰阶模板升级为 Indigo/Slate 专业管理后台，新增 Overview 总览页，重构 Cron Jobs 与 Settings 页面，统一共享组件与交互状态。

**Architecture:** 先更新设计系统 token 与布局壳，再沉淀共享组件，最后拼装页面。所有数据仍复用现有 TanStack Query hooks，Overview 的统计数字由前端聚合计算。新增少量 shadcn 组件（Card、DropdownMenu）并补充前端单元测试。

**Tech Stack:** React 19 + TypeScript 5.6 + Vite 6 + Tailwind CSS 3.4 + shadcn/ui + TanStack Query 5 + Vitest 2.1 + Testing Library.

---

## File Structure

### 新增 shadcn/ui 组件
- `web/src/components/ui/card.tsx` — 卡片容器（StatsCard、页面卡片）。
- `web/src/components/ui/dropdown-menu.tsx` — 表格行操作下拉菜单。

### 新增共享组件
- `web/src/components/page-header.tsx` — 统一页面标题区。
- `web/src/components/stats-card.tsx` — 统计卡片。
- `web/src/components/empty-state.tsx` — 空状态。
- `web/src/components/status-badge.tsx` — 状态标签。
- `web/src/components/relative-time.tsx` — 相对时间 + tooltip。
- `web/src/components/search-input.tsx` — 搜索输入框。
- `web/src/components/data-table-toolbar.tsx` — 表格工具栏布局容器。
- `web/src/components/page-header-skeleton.tsx` — 页面标题骨架。
- `web/src/components/stats-cards-skeleton.tsx` — 统计卡片骨架。
- `web/src/components/table-skeleton.tsx` — 表格骨架。

### 新增 Hooks
- `web/src/hooks/use-cron-stats.ts` — Cron 任务统计 hook。

### 新增页面
- `web/src/pages/overview.tsx` — 总览页。

### 新增测试
- `web/src/components/__tests__/page-header.test.tsx`
- `web/src/components/__tests__/stats-card.test.tsx`
- `web/src/components/__tests__/empty-state.test.tsx`
- `web/src/components/__tests__/status-badge.test.tsx`
- `web/src/components/__tests__/relative-time.test.tsx`
- `web/src/components/__tests__/search-input.test.tsx`

### 修改文件
- `web/package.json` — 新增 `@radix-ui/react-dropdown-menu` 依赖。
- `web/vite.config.ts` — 增加 Vitest `environment: "jsdom"` 与 alias。
- `web/tailwind.config.ts` — 扩展 success / warning 颜色。
- `web/src/index.css` — 更新设计 token。
- `web/src/App.tsx` — 新增 overview 路由。
- `web/src/components/layout.tsx` — Sidebar、Header 重构。
- `web/src/pages/cron-jobs.tsx` — 接入新组件。
- `web/src/pages/settings.tsx` — 接入新组件。
- `web/src/components/cron-jobs/CronJobTable.tsx` — 新表格。
- `web/src/components/cron-jobs/CronJobEditDialog.tsx` — 样式统一。
- `web/src/components/cron-jobs/CronJobDeleteDialog.tsx` — 样式统一。
- `web/src/components/settings/SettingsTable.tsx` — 新表格。
- `web/src/components/settings/SettingEditDialog.tsx` — 样式统一。
- `web/src/components/page-skeleton.tsx` — 调整或删除。

---

## Task 1: Add missing shadcn/ui dependencies

**Files:**
- Modify: `web/package.json`

**Context:** `dropdown-menu.tsx` 需要 `@radix-ui/react-dropdown-menu`，项目当前未安装。

- [ ] **Step 1: Add dependency**

在 `web/package.json` 的 `dependencies` 中加入：

```json
"@radix-ui/react-dropdown-menu": "^2.1.2",
```

- [ ] **Step 2: Install**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm install
```

Expected: `node_modules/@radix-ui/react-dropdown-menu` 存在。

- [ ] **Step 3: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml
git commit -m "chore(web): add @radix-ui/react-dropdown-menu dependency"
```

---

## Task 2: Configure Vitest environment

**Files:**
- Modify: `web/vite.config.ts`

**Context:** 前端测试使用 Vitest + jsdom，但当前未配置 test environment。

- [ ] **Step 1: Update vite.config.ts**

完整替换为：

```ts
import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
	plugins: [react()],
	resolve: {
		alias: {
			"@": path.resolve(__dirname, "./src"),
		},
	},
	server: {
		proxy: {
			"/api": {
				target: "http://localhost:4007",
				changeOrigin: true,
			},
		},
	},
	build: {
		outDir: "dist",
		sourcemap: false,
		rollupOptions: {
			output: {
				manualChunks(id) {
					if (id.includes("node_modules/react-router-dom")) return "router";
					if (id.includes("node_modules/@tanstack/react-query")) return "query";
					if (id.includes("node_modules/@radix-ui")) return "ui";
					if (id.includes("node_modules/lucide-react")) return "icons";
					if (id.includes("node_modules/react-dom") || id.includes("node_modules/react/")) {
						return "react-vendor";
					}
				},
			},
		},
	},
	test: {
		environment: "jsdom",
		globals: true,
		setupFiles: ["./src/test/setup.ts"],
	},
});
```

- [ ] **Step 2: Create test setup file**

Create: `web/src/test/setup.ts`

```ts
import "@testing-library/jest-dom/vitest";
```

- [ ] **Step 3: Verify config**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run
```

Expected: Vitest starts, finds no tests yet, exits 0.

- [ ] **Step 4: Commit**

```bash
git add web/vite.config.ts web/src/test/setup.ts
git commit -m "chore(web): configure vitest with jsdom"
```

---

## Task 3: Add shadcn/ui Card component

**Files:**
- Create: `web/src/components/ui/card.tsx`

- [ ] **Step 1: Create card.tsx**

```tsx
import * as React from "react";

import { cn } from "@/lib/utils";

const Card = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
	({ className, ...props }, ref) => (
		<div
			ref={ref}
			className={cn(
				"rounded-xl border bg-card text-card-foreground shadow-sm",
				className,
			)}
			{...props}
		/>
	),
);
Card.displayName = "Card";

const CardHeader = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
	({ className, ...props }, ref) => (
		<div ref={ref} className={cn("flex flex-col space-y-1.5 p-6", className)} {...props} />
	),
);
CardHeader.displayName = "CardHeader";

const CardTitle = React.forwardRef<HTMLParagraphElement, React.HTMLAttributes<HTMLHeadingElement>>(
	({ className, ...props }, ref) => (
		<h3
			ref={ref}
			className={cn("text-lg font-semibold leading-none tracking-tight", className)}
			{...props}
		/>
	),
);
CardTitle.displayName = "CardTitle";

const CardDescription = React.forwardRef<
	HTMLParagraphElement,
	React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
	<p ref={ref} className={cn("text-sm text-muted-foreground", className)} {...props} />
));
CardDescription.displayName = "CardDescription";

const CardContent = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
	({ className, ...props }, ref) => <div ref={ref} className={cn("p-6 pt-0", className)} {...props} />,
);
CardContent.displayName = "CardContent";

const CardFooter = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
	({ className, ...props }, ref) => (
		<div ref={ref} className={cn("flex items-center p-6 pt-0", className)} {...props} />
	),
);
CardFooter.displayName = "CardFooter";

export { Card, CardHeader, CardFooter, CardTitle, CardDescription, CardContent };
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/ui/card.tsx
git commit -m "feat(web): add shadcn card component"
```

---

## Task 4: Add shadcn/ui DropdownMenu component

**Files:**
- Create: `web/src/components/ui/dropdown-menu.tsx`

- [ ] **Step 1: Create dropdown-menu.tsx**

```tsx
import * as DropdownMenuPrimitive from "@radix-ui/react-dropdown-menu";
import { Check, ChevronRight, Circle } from "lucide-react";
import * as React from "react";

import { cn } from "@/lib/utils";

const DropdownMenu = DropdownMenuPrimitive.Root;
const DropdownMenuTrigger = DropdownMenuPrimitive.Trigger;
const DropdownMenuGroup = DropdownMenuPrimitive.Group;
const DropdownMenuPortal = DropdownMenuPrimitive.Portal;
const DropdownMenuSub = DropdownMenuPrimitive.Sub;
const DropdownMenuRadioGroup = DropdownMenuPrimitive.RadioGroup;

const DropdownMenuSubTrigger = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.SubTrigger>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.SubTrigger> & {
		inset?: boolean;
	}
>(({ className, inset, children, ...props }, ref) => (
	<DropdownMenuPrimitive.SubTrigger
		ref={ref}
		className={cn(
			"flex cursor-default select-none items-center rounded-sm px-2 py-1.5 text-sm outline-none focus:bg-accent data-[state=open]:bg-accent",
			inset && "pl-8",
			className,
		)}
		{...props}
	>
		{children}
		<ChevronRight className="ml-auto size-4" />
	</DropdownMenuPrimitive.SubTrigger>
));
DropdownMenuSubTrigger.displayName = DropdownMenuPrimitive.SubTrigger.displayName;

const DropdownMenuSubContent = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.SubContent>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.SubContent>
>(({ className, ...props }, ref) => (
	<DropdownMenuPrimitive.SubContent
		ref={ref}
		className={cn(
			"z-50 min-w-[8rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-lg data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
			className,
		)}
		{...props}
	/>
));
DropdownMenuSubContent.displayName = DropdownMenuPrimitive.SubContent.displayName;

const DropdownMenuContent = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.Content>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Content>
>(({ className, sideOffset = 4, ...props }, ref) => (
	<DropdownMenuPrimitive.Portal>
		<DropdownMenuPrimitive.Content
			ref={ref}
			sideOffset={sideOffset}
			className={cn(
				"z-50 min-w-[8rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-md data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2",
				className,
			)}
			{...props}
		/>
	</DropdownMenuPrimitive.Portal>
));
DropdownMenuContent.displayName = DropdownMenuPrimitive.Content.displayName;

const DropdownMenuItem = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.Item>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Item> & {
		inset?: boolean;
	}
>(({ className, inset, ...props }, ref) => (
	<DropdownMenuPrimitive.Item
		ref={ref}
		className={cn(
			"relative flex cursor-default select-none items-center rounded-sm px-2 py-1.5 text-sm outline-none transition-colors focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
			inset && "pl-8",
			className,
		)}
		{...props}
	/>
));
DropdownMenuItem.displayName = DropdownMenuPrimitive.Item.displayName;

const DropdownMenuCheckboxItem = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.CheckboxItem>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.CheckboxItem>
>(({ className, children, checked, ...props }, ref) => (
	<DropdownMenuPrimitive.CheckboxItem
		ref={ref}
		className={cn(
			"relative flex cursor-default select-none items-center rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none transition-colors focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
			className,
		)}
		checked={checked}
		{...props}
	>
		<span className="absolute left-2 flex size-3.5 items-center justify-center">
			<DropdownMenuPrimitive.ItemIndicator>
				<Check className="size-4" />
			</DropdownMenuPrimitive.ItemIndicator>
		</span>
		{children}
	</DropdownMenuPrimitive.CheckboxItem>
));
DropdownMenuCheckboxItem.displayName = DropdownMenuPrimitive.CheckboxItem.displayName;

const DropdownMenuRadioItem = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.RadioItem>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.RadioItem>
>(({ className, children, ...props }, ref) => (
	<DropdownMenuPrimitive.RadioItem
		ref={ref}
		className={cn(
			"relative flex cursor-default select-none items-center rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none transition-colors focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
			className,
		)}
		{...props}
	>
		<span className="absolute left-2 flex size-3.5 items-center justify-center">
			<DropdownMenuPrimitive.ItemIndicator>
				<Circle className="size-2 fill-current" />
			</DropdownMenuPrimitive.ItemIndicator>
		</span>
		{children}
	</DropdownMenuPrimitive.RadioItem>
));
DropdownMenuRadioItem.displayName = DropdownMenuPrimitive.RadioItem.displayName;

const DropdownMenuLabel = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.Label>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Label> & {
		inset?: boolean;
	}
>(({ className, inset, ...props }, ref) => (
	<DropdownMenuPrimitive.Label
		ref={ref}
		className={cn("px-2 py-1.5 text-sm font-semibold", inset && "pl-8", className)}
		{...props}
	/>
));
DropdownMenuLabel.displayName = DropdownMenuPrimitive.Label.displayName;

const DropdownMenuSeparator = React.forwardRef<
	React.ElementRef<typeof DropdownMenuPrimitive.Separator>,
	React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Separator>
>(({ className, ...props }, ref) => (
	<DropdownMenuPrimitive.Separator
		ref={ref}
		className={cn("-mx-1 my-1 h-px bg-muted", className)}
		{...props}
	/>
));
DropdownMenuSeparator.displayName = DropdownMenuPrimitive.Separator.displayName;

const DropdownMenuShortcut = ({ className, ...props }: React.HTMLAttributes<HTMLSpanElement>) => {
	return (
		<span className={cn("ml-auto text-xs tracking-widest opacity-60", className)} {...props} />
	);
};
DropdownMenuShortcut.displayName = "DropdownMenuShortcut";

export {
	DropdownMenu,
	DropdownMenuTrigger,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuCheckboxItem,
	DropdownMenuRadioItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuShortcut,
	DropdownMenuGroup,
	DropdownMenuPortal,
	DropdownMenuSub,
	DropdownMenuSubContent,
	DropdownMenuSubTrigger,
	DropdownMenuRadioGroup,
};
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/ui/dropdown-menu.tsx
git commit -m "feat(web): add shadcn dropdown-menu component"
```

---

## Task 5: Update design tokens in index.css

**Files:**
- Modify: `web/src/index.css`

**Context:** 将默认灰阶主题替换为 Indigo/Slate 设计系统，并加入 success / warning 语义色变量。

- [ ] **Step 1: Replace CSS variables**

完整替换 `:root` 与 `.dark` 内容（保留 `@tailwind` 指令与底部动画注释）：

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

@layer base {
	:root {
		--background: 210 40% 98%;
		--foreground: 215 25% 17%;
		--card: 0 0% 100%;
		--card-foreground: 215 25% 17%;
		--popover: 0 0% 100%;
		--popover-foreground: 215 25% 17%;
		--primary: 243 75% 59%;
		--primary-foreground: 0 0% 100%;
		--secondary: 210 40% 96%;
		--secondary-foreground: 215 25% 17%;
		--muted: 210 40% 96%;
		--muted-foreground: 215 16% 47%;
		--accent: 243 75% 97%;
		--accent-foreground: 243 75% 35%;
		--destructive: 0 72% 51%;
		--destructive-foreground: 0 0% 100%;
		--success: 160 84% 39%;
		--success-foreground: 0 0% 100%;
		--warning: 38 92% 50%;
		--warning-foreground: 0 0% 100%;
		--border: 214 32% 91%;
		--input: 214 32% 91%;
		--ring: 243 75% 59%;
		--radius: 0.75rem;
		--sidebar-background: 0 0% 100%;
		--sidebar-foreground: 215 25% 17%;
		--sidebar-primary: 243 75% 59%;
		--sidebar-primary-foreground: 0 0% 100%;
		--sidebar-accent: 243 75% 97%;
		--sidebar-accent-foreground: 243 75% 35%;
		--sidebar-border: 214 32% 91%;
		--sidebar-ring: 243 75% 59%;
	}

	.dark {
		--background: 222 47% 6%;
		--foreground: 210 40% 98%;
		--card: 222 47% 8%;
		--card-foreground: 210 40% 98%;
		--popover: 222 47% 8%;
		--popover-foreground: 210 40% 98%;
		--primary: 243 75% 65%;
		--primary-foreground: 222 47% 6%;
		--secondary: 217 33% 17%;
		--secondary-foreground: 210 40% 98%;
		--muted: 217 33% 17%;
		--muted-foreground: 215 20% 65%;
		--accent: 243 50% 18%;
		--accent-foreground: 243 75% 90%;
		--destructive: 0 63% 31%;
		--destructive-foreground: 210 40% 98%;
		--success: 160 84% 45%;
		--success-foreground: 222 47% 6%;
		--warning: 38 92% 55%;
		--warning-foreground: 222 47% 6%;
		--border: 217 33% 17%;
		--input: 217 33% 17%;
		--ring: 243 75% 65%;
		--sidebar-background: 222 47% 8%;
		--sidebar-foreground: 210 40% 98%;
		--sidebar-primary: 243 75% 65%;
		--sidebar-primary-foreground: 0 0% 100%;
		--sidebar-accent: 243 50% 18%;
		--sidebar-accent-foreground: 243 75% 90%;
		--sidebar-border: 217 33% 17%;
		--sidebar-ring: 243 75% 65%;
	}
}

@layer base {
	* {
		@apply border-border;
	}
	body {
		@apply bg-background text-foreground;
	}
}

/* View Transition animation for theme toggle — controlled by JS-injected <style> */
```

- [ ] **Step 2: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds with no TypeScript errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/index.css
git commit -m "feat(web): update design tokens to indigo/slate theme"
```

---

## Task 6: Extend Tailwind config with semantic colors

**Files:**
- Modify: `web/tailwind.config.ts`

- [ ] **Step 1: Add success and warning colors**

在 `colors` 对象内 `destructive` 之后添加：

```ts
success: {
	DEFAULT: "hsl(var(--success))",
	foreground: "hsl(var(--success-foreground))",
},
warning: {
	DEFAULT: "hsl(var(--warning))",
	foreground: "hsl(var(--warning-foreground))",
},
```

- [ ] **Step 2: Commit**

```bash
git add web/tailwind.config.ts
git commit -m "feat(web): extend tailwind with success/warning colors"
```

---

## Task 7: Create PageHeader shared component

**Files:**
- Create: `web/src/components/page-header.tsx`
- Create: `web/src/components/page-header-skeleton.tsx`
- Create: `web/src/components/__tests__/page-header.test.tsx`

- [ ] **Step 1: Create page-header.tsx**

```tsx
import type { LucideIcon } from "lucide-react";

interface PageHeaderProps {
	icon?: LucideIcon;
	title: string;
	description?: string;
	children?: React.ReactNode;
}

export function PageHeader({ icon: Icon, title, description, children }: PageHeaderProps) {
	return (
		<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
			<div className="flex items-start gap-3">
				{Icon && (
					<div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
						<Icon className="size-5" />
					</div>
				)}
				<div>
					<h1 className="text-3xl font-bold tracking-tight">{title}</h1>
					{description && <p className="text-base text-muted-foreground">{description}</p>}
				</div>
			</div>
			{children && <div className="flex items-center gap-2">{children}</div>}
		</div>
	);
}
```

- [ ] **Step 2: Create page-header-skeleton.tsx**

```tsx
import { Skeleton } from "@/components/ui/skeleton";

export function PageHeaderSkeleton() {
	return (
		<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
			<div className="flex items-start gap-3">
				<Skeleton className="size-10 rounded-xl" />
				<div className="space-y-2">
					<Skeleton className="h-9 w-48" />
					<Skeleton className="h-5 w-64" />
				</div>
			</div>
		</div>
	);
}
```

- [ ] **Step 3: Create test**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Clock } from "lucide-react";
import { PageHeader } from "@/components/page-header";

describe("PageHeader", () => {
	it("renders title and description", () => {
		render(<PageHeader title="定时任务" description="管理定时任务" />);
		expect(screen.getByText("定时任务")).toBeInTheDocument();
		expect(screen.getByText("管理定时任务")).toBeInTheDocument();
	});

	it("renders icon and children", () => {
		render(
			<PageHeader title="设置" icon={Clock}>
				<button type="button">操作</button>
			</PageHeader>,
		);
		expect(screen.getByText("设置")).toBeInTheDocument();
		expect(screen.getByText("操作")).toBeInTheDocument();
	});
});
```

- [ ] **Step 4: Run test**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run page-header
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/page-header.tsx web/src/components/page-header-skeleton.tsx web/src/components/__tests__/page-header.test.tsx
git commit -m "feat(web): add PageHeader component with skeleton and tests"
```

---

## Task 8: Create StatsCard shared component

**Files:**
- Create: `web/src/components/stats-card.tsx`
- Create: `web/src/components/stats-cards-skeleton.tsx`
- Create: `web/src/components/__tests__/stats-card.test.tsx`

- [ ] **Step 1: Create stats-card.tsx**

```tsx
import { Card, CardContent } from "@/components/ui/card";
import type { LucideIcon } from "lucide-react";

interface StatsCardProps {
	icon: LucideIcon;
	label: string;
	value: React.ReactNode;
	subLabel?: string;
}

export function StatsCard({ icon: Icon, label, value, subLabel }: StatsCardProps) {
	return (
		<Card>
			<CardContent className="flex items-start gap-4 p-6">
				<div className="flex size-10 items-center justify-center rounded-xl bg-primary/10 text-primary">
					<Icon className="size-5" />
				</div>
				<div className="min-w-0">
					<p className="text-sm font-medium text-muted-foreground">{label}</p>
					<p className="text-2xl font-bold">{value}</p>
					{subLabel && <p className="text-xs text-muted-foreground">{subLabel}</p>}
				</div>
			</CardContent>
		</Card>
	);
}
```

- [ ] **Step 2: Create stats-cards-skeleton.tsx**

```tsx
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";

export function StatsCardsSkeleton({ count = 4 }: { count?: number }) {
	return (
		<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
			{Array.from({ length: count }).map((_, i) => (
				<Card key={i}>
					<CardContent className="flex items-start gap-4 p-6">
						<Skeleton className="size-10 rounded-xl" />
						<div className="space-y-2">
							<Skeleton className="h-4 w-20" />
							<Skeleton className="h-8 w-16" />
						</div>
					</CardContent>
				</Card>
			))}
		</div>
	);
}
```

- [ ] **Step 3: Create test**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Clock } from "lucide-react";
import { StatsCard } from "@/components/stats-card";

describe("StatsCard", () => {
	it("renders label, value and subLabel", () => {
		render(<StatsCard icon={Clock} label="总任务" value={12} subLabel="全部任务" />);
		expect(screen.getByText("总任务")).toBeInTheDocument();
		expect(screen.getByText("12")).toBeInTheDocument();
		expect(screen.getByText("全部任务")).toBeInTheDocument();
	});
});
```

- [ ] **Step 4: Run test**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run stats-card
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/stats-card.tsx web/src/components/stats-cards-skeleton.tsx web/src/components/__tests__/stats-card.test.tsx
git commit -m "feat(web): add StatsCard component with skeleton and tests"
```

---

## Task 9: Create EmptyState shared component

**Files:**
- Create: `web/src/components/empty-state.tsx`
- Create: `web/src/components/__tests__/empty-state.test.tsx`

- [ ] **Step 1: Create empty-state.tsx**

```tsx
import type { LucideIcon } from "lucide-react";

interface EmptyStateProps {
	icon?: LucideIcon;
	title: string;
	description?: string;
	action?: React.ReactNode;
}

export function EmptyState({ icon: Icon, title, description, action }: EmptyStateProps) {
	return (
		<div className="flex flex-col items-center justify-center rounded-xl border bg-card p-8 text-center shadow-sm">
			{Icon && <Icon className="mb-4 size-12 text-muted-foreground/50" />}
			<h3 className="text-lg font-semibold">{title}</h3>
			{description && <p className="mt-1 max-w-sm text-sm text-muted-foreground">{description}</p>}
			{action && <div className="mt-4">{action}</div>}
		</div>
	);
}
```

- [ ] **Step 2: Create test**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Inbox } from "lucide-react";
import { EmptyState } from "@/components/empty-state";

describe("EmptyState", () => {
	it("renders title, description and action", () => {
		render(
			<EmptyState
				icon={Inbox}
				title="暂无数据"
				description="当前没有任何记录"
				action={<button type="button">新建</button>}
			/>,
		);
		expect(screen.getByText("暂无数据")).toBeInTheDocument();
		expect(screen.getByText("当前没有任何记录")).toBeInTheDocument();
		expect(screen.getByText("新建")).toBeInTheDocument();
	});
});
```

- [ ] **Step 3: Run test**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run empty-state
```

Expected: 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/empty-state.tsx web/src/components/__tests__/empty-state.test.tsx
git commit -m "feat(web): add EmptyState component and tests"
```

---

## Task 10: Create StatusBadge shared component

**Files:**
- Create: `web/src/components/status-badge.tsx`
- Create: `web/src/components/__tests__/status-badge.test.tsx`

- [ ] **Step 1: Create status-badge.tsx**

```tsx
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

interface StatusBadgeProps {
	status: "enabled" | "disabled" | "success" | "error" | "warning";
	label?: string;
}

const variants: Record<StatusBadgeProps["status"], string> = {
	enabled:
		"bg-emerald-100 text-emerald-700 hover:bg-emerald-100 dark:bg-emerald-900/30 dark:text-emerald-400",
	disabled:
		"bg-slate-100 text-slate-700 hover:bg-slate-100 dark:bg-slate-800 dark:text-slate-400",
	success:
		"bg-emerald-100 text-emerald-700 hover:bg-emerald-100 dark:bg-emerald-900/30 dark:text-emerald-400",
	error: "bg-red-100 text-red-700 hover:bg-red-100 dark:bg-red-900/30 dark:text-red-400",
	warning:
		"bg-amber-100 text-amber-700 hover:bg-amber-100 dark:bg-amber-900/30 dark:text-amber-400",
};

const defaultLabels: Record<StatusBadgeProps["status"], string> = {
	enabled: "启用",
	disabled: "禁用",
	success: "成功",
	error: "失败",
	warning: "警告",
};

export function StatusBadge({ status, label }: StatusBadgeProps) {
	return <Badge className={cn(variants[status])}>{label ?? defaultLabels[status]}</Badge>;
}
```

- [ ] **Step 2: Create test**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StatusBadge } from "@/components/status-badge";

describe("StatusBadge", () => {
	it("renders default enabled label", () => {
		render(<StatusBadge status="enabled" />);
		expect(screen.getByText("启用")).toBeInTheDocument();
	});

	it("renders custom label", () => {
		render(<StatusBadge status="disabled" label="已停用" />);
		expect(screen.getByText("已停用")).toBeInTheDocument();
	});
});
```

- [ ] **Step 3: Run test**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run status-badge
```

Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/status-badge.tsx web/src/components/__tests__/status-badge.test.tsx
git commit -m "feat(web): add StatusBadge component and tests"
```

---

## Task 11: Create RelativeTime shared component

**Files:**
- Create: `web/src/components/relative-time.tsx`
- Create: `web/src/components/__tests__/relative-time.test.tsx`

- [ ] **Step 1: Create relative-time.tsx**

```tsx
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";

interface RelativeTimeProps {
	date: string | Date;
	fallback?: string;
}

function formatRelativeTime(date: Date): string {
	const now = new Date();
	const diffMs = now.getTime() - date.getTime();
	const diffSec = Math.floor(diffMs / 1000);
	if (diffSec < 60) return "刚刚";
	const diffMin = Math.floor(diffSec / 60);
	if (diffMin < 60) return `${diffMin} 分钟前`;
	const diffHour = Math.floor(diffMin / 60);
	if (diffHour < 24) return `${diffHour} 小时前`;
	const diffDay = Math.floor(diffHour / 24);
	if (diffDay < 30) return `${diffDay} 天前`;
	return date.toLocaleDateString("zh-CN");
}

export function RelativeTime({ date, fallback = "—" }: RelativeTimeProps) {
	const parsed = typeof date === "string" ? new Date(date) : date;
	const ts = parsed.getTime();
	if (Number.isNaN(ts) || ts <= 0) {
		return <span className="text-muted-foreground">{fallback}</span>;
	}

	const full = parsed.toLocaleString("zh-CN");
	const relative = formatRelativeTime(parsed);

	return (
		<TooltipProvider>
			<Tooltip>
				<TooltipTrigger asChild>
					<span className="cursor-help">{relative}</span>
				</TooltipTrigger>
				<TooltipContent>{full}</TooltipContent>
			</Tooltip>
		</TooltipProvider>
	);
}
```

- [ ] **Step 2: Create test**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RelativeTime } from "@/components/relative-time";

describe("RelativeTime", () => {
	it("renders fallback for invalid date", () => {
		render(<RelativeTime date="" fallback="等待执行" />);
		expect(screen.getByText("等待执行")).toBeInTheDocument();
	});

	it("renders relative time for recent date", () => {
		const recent = new Date(Date.now() - 2 * 60 * 1000).toISOString();
		render(<RelativeTime date={recent} />);
		expect(screen.getByText("2 分钟前")).toBeInTheDocument();
	});
});
```

- [ ] **Step 3: Run test**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run relative-time
```

Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/relative-time.tsx web/src/components/__tests__/relative-time.test.tsx
git commit -m "feat(web): add RelativeTime component and tests"
```

---

## Task 12: Create SearchInput and DataTableToolbar

**Files:**
- Create: `web/src/components/search-input.tsx`
- Create: `web/src/components/data-table-toolbar.tsx`
- Create: `web/src/components/__tests__/search-input.test.tsx`

- [ ] **Step 1: Create search-input.tsx**

```tsx
import { Input } from "@/components/ui/input";
import { Search } from "lucide-react";

interface SearchInputProps {
	value: string;
	onChange: (value: string) => void;
	placeholder?: string;
}

export function SearchInput({ value, onChange, placeholder = "搜索..." }: SearchInputProps) {
	return (
		<div className="relative">
			<Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
			<Input
				type="search"
				placeholder={placeholder}
				value={value}
				onChange={(e) => onChange(e.target.value)}
				className="pl-9"
			/>
		</div>
	);
}
```

- [ ] **Step 2: Create data-table-toolbar.tsx**

```tsx
interface DataTableToolbarProps {
	children: React.ReactNode;
}

export function DataTableToolbar({ children }: DataTableToolbarProps) {
	return (
		<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
			{children}
		</div>
	);
}
```

- [ ] **Step 3: Create test**

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SearchInput } from "@/components/search-input";

describe("SearchInput", () => {
	it("calls onChange when user types", () => {
		const handleChange = vi.fn();
		render(<SearchInput value="" onChange={handleChange} />);
		const input = screen.getByPlaceholderText("搜索...");
		fireEvent.change(input, { target: { value: "hello" } });
		expect(handleChange).toHaveBeenCalledWith("hello");
	});
});
```

- [ ] **Step 4: Run test**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run search-input
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/search-input.tsx web/src/components/data-table-toolbar.tsx web/src/components/__tests__/search-input.test.tsx
git commit -m "feat(web): add SearchInput and DataTableToolbar components"
```

---

## Task 13: Create TableSkeleton

**Files:**
- Create: `web/src/components/table-skeleton.tsx`

- [ ] **Step 1: Create table-skeleton.tsx**

```tsx
import { Skeleton } from "@/components/ui/skeleton";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";

interface TableSkeletonProps {
	columns: number;
	rows?: number;
}

export function TableSkeleton({ columns, rows = 5 }: TableSkeletonProps) {
	return (
		<div className="rounded-xl border bg-card shadow-sm">
			<Table>
				<TableHeader>
					<TableRow>
						{Array.from({ length: columns }).map((_, i) => (
							<TableHead key={i}>
								<Skeleton className="h-4 w-20" />
							</TableHead>
						))}
					</TableRow>
				</TableHeader>
				<TableBody>
					{Array.from({ length: rows }).map((_, rowIndex) => (
						<TableRow key={rowIndex}>
							{Array.from({ length: columns }).map((_, colIndex) => (
								<TableCell key={colIndex}>
									<Skeleton className="h-4 w-full max-w-[120px]" />
								</TableCell>
							))}
						</TableRow>
					))}
				</TableBody>
			</Table>
		</div>
	);
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/table-skeleton.tsx
git commit -m "feat(web): add TableSkeleton component"
```

---

## Task 14: Refactor AppLayout (Sidebar + Header)

**Files:**
- Modify: `web/src/components/layout.tsx`

**Context:** 更新 Sidebar 品牌区、增加底部环境信息，Header 增加页面标题、搜索和操作按钮。

- [ ] **Step 1: Update layout.tsx**

完整替换为：

```tsx
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
	Sidebar,
	SidebarContent,
	SidebarFooter,
	SidebarGroup,
	SidebarGroupContent,
	SidebarGroupLabel,
	SidebarHeader,
	SidebarInset,
	SidebarMenu,
	SidebarMenuButton,
	SidebarMenuItem,
	SidebarProvider,
	SidebarTrigger,
} from "@/components/ui/sidebar";
import { cn } from "@/lib/utils";
import { Clock, LayoutDashboard, RefreshCw, Search, Settings } from "lucide-react";
import { Suspense } from "react";
import { Link, Outlet, useLocation } from "react-router-dom";

const navItems = [
	{
		title: "总览",
		url: "/",
		icon: LayoutDashboard,
	},
	{
		title: "定时任务",
		url: "/cron-jobs",
		icon: Clock,
	},
	{
		title: "设置",
		url: "/settings",
		icon: Settings,
	},
];

const pageTitles: Record<string, { title: string; description?: string }> = {
	"/": { title: "总览", description: "系统概览与最近动态" },
	"/cron-jobs": { title: "定时任务", description: "管理系统定时任务" },
	"/settings": { title: "系统设置", description: "管理应用配置项" },
};

export default function AppLayout() {
	const location = useLocation();
	const currentPage = pageTitles[location.pathname] ?? { title: "" };

	return (
		<SidebarProvider>
			<Sidebar>
				<SidebarHeader>
					<SidebarMenu>
						<SidebarMenuItem>
							<SidebarMenuButton size="lg" asChild>
								<Link to="/">
									<div className="flex aspect-square size-10 items-center justify-center rounded-xl bg-primary text-primary-foreground">
										<Settings className="size-5" />
									</div>
									<div className="flex flex-col gap-0.5 leading-none">
										<span className="font-semibold">RS Template</span>
										<span className="text-xs text-muted-foreground">管理后台</span>
									</div>
								</Link>
							</SidebarMenuButton>
						</SidebarMenuItem>
					</SidebarMenu>
				</SidebarHeader>
				<SidebarContent>
					<SidebarGroup>
						<SidebarGroupLabel>导航</SidebarGroupLabel>
						<SidebarGroupContent>
							<SidebarMenu>
								{navItems.map((item) => (
									<SidebarMenuItem key={item.url}>
										<SidebarMenuButton
											asChild
											isActive={location.pathname === item.url}
											className={cn(
												location.pathname === item.url &&
													"bg-sidebar-accent text-sidebar-accent-foreground",
											)}
										>
											<Link to={item.url}>
												<item.icon />
												<span>{item.title}</span>
											</Link>
										</SidebarMenuButton>
									</SidebarMenuItem>
								))}
							</SidebarMenu>
						</SidebarGroupContent>
					</SidebarGroup>
				</SidebarContent>
				<SidebarFooter>
					<div className="px-4 py-2 text-xs text-muted-foreground">
						<div>RS Template v0.1.0</div>
					</div>
				</SidebarFooter>
			</Sidebar>
			<SidebarInset>
				<header className="flex h-16 shrink-0 items-center gap-4 border-b bg-card px-6">
					<SidebarTrigger className="-ml-2" />
					<Separator orientation="vertical" className="h-6" />
					<div className="flex flex-1 items-center justify-between gap-4">
						<div>
							<h2 className="text-lg font-semibold">{currentPage.title}</h2>
							{currentPage.description && (
								<p className="hidden text-xs text-muted-foreground sm:block">
									{currentPage.description}
								</p>
							)}
						</div>
						<div className="flex items-center gap-2">
							<div className="relative hidden md:block">
								<Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
								<input
									type="search"
									placeholder="全局搜索（占位）"
									className="h-9 rounded-md border border-input bg-background px-9 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
									disabled
								/>
							</div>
							<Button variant="outline" size="icon" title="刷新">
								<RefreshCw className="size-4" />
							</Button>
							<ThemeToggle />
						</div>
					</div>
				</header>
				<div className="mx-auto flex w-full max-w-7xl flex-1 flex-col gap-6 p-6">
					<Suspense
						fallback={
							<div className="flex flex-1 items-center justify-center text-muted-foreground">
								页面加载中...
							</div>
						}
					>
						<Outlet />
					</Suspense>
				</div>
			</SidebarInset>
		</SidebarProvider>
	);
}
```

- [ ] **Step 2: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/layout.tsx
git commit -m "feat(web): refactor layout with new sidebar, header and page title"
```

---

## Task 15: Create shared useCronStats hook and Overview page

**Files:**
- Create: `web/src/hooks/use-cron-stats.ts`
- Create: `web/src/pages/overview.tsx`
- Modify: `web/src/App.tsx`

- [ ] **Step 1: Create use-cron-stats.ts**

```tsx
import { type CronJob, useCronJobs } from "@/hooks/use-cron-jobs";
import { useMemo } from "react";

export interface CronStats {
	total: number;
	enabled: number;
	groups: number;
	lastRun: CronJob | undefined;
}

export function useCronStats(): CronStats {
	const { data: jobs } = useCronJobs();
	return useMemo(() => {
		const all = jobs ?? [];
		const enabled = all.filter((j) => j.enabled).length;
		const groups = new Set(all.map((j) => j.group || "默认")).size;
		const lastRun = all
			.filter((j) => j.last_run_at && new Date(j.last_run_at).getTime() > 0)
			.sort((a, b) => new Date(b.last_run_at).getTime() - new Date(a.last_run_at).getTime())[0];
		return { total: all.length, enabled, groups, lastRun };
	}, [jobs]);
}
```

- [ ] **Step 2: Create overview.tsx**

```tsx
import { EmptyState } from "@/components/empty-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { RelativeTime } from "@/components/relative-time";
import { useCronStats } from "@/hooks/use-cron-stats";
import { useCronJobs } from "@/hooks/use-cron-jobs";
import { type Setting, useSettings } from "@/hooks/use-settings";
import { Activity, Clock, Play, Settings } from "lucide-react";
import { useMemo } from "react";

function useRecentSettings(settings: Setting[] | undefined) {
	return useMemo(() => {
		return [...(settings ?? [])]
			.filter((s) => s.updated_at)
			.sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
			.slice(0, 5);
	}, [settings]);
}

function useRecentJobs(jobs: CronJob[] | undefined) {
	return useMemo(() => {
		return [...(jobs ?? [])]
			.filter((j) => j.last_run_at && new Date(j.last_run_at).getTime() > 0)
			.sort((a, b) => new Date(b.last_run_at).getTime() - new Date(a.last_run_at).getTime())
			.slice(0, 5);
	}, [jobs]);
}

export default function OverviewPage() {
	const { data: jobs, isLoading: jobsLoading } = useCronJobs();
	const { data: settings, isLoading: settingsLoading } = useSettings();

	const stats = useCronStats();
	const recentJobs = useRecentJobs(jobs);
	const recentSettings = useRecentSettings(settings);
	const isLoading = jobsLoading || settingsLoading;

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<StatsCardsSkeleton count={4} />
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader title="总览" description="系统概览与最近动态" />

			<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
				<StatsCard icon={Clock} label="定时任务总数" value={stats.total} subLabel="全部任务" />
				<StatsCard icon={Play} label="已启用任务" value={stats.enabled} subLabel="运行中" />
				<StatsCard
					icon={Settings}
					label="系统设置项"
					value={settings?.length ?? 0}
					subLabel="配置项"
				/>
				<StatsCard
					icon={Activity}
					label="最近执行"
					value={stats.lastRun ? <RelativeTime date={stats.lastRun.last_run_at} /> : "—"}
					subLabel="最近一次 Cron 执行"
				/>
			</div>

			<div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
				<Card>
					<CardHeader>
						<CardTitle>最近执行的 Cron 任务</CardTitle>
					</CardHeader>
					<CardContent>
						{recentJobs.length > 0 ? (
							<ul className="space-y-3">
								{recentJobs.map((job) => (
									<li key={job.name} className="flex items-center justify-between text-sm">
										<div>
											<p className="font-medium">{job.name}</p>
											<p className="text-xs text-muted-foreground">{job.group || "默认"}</p>
										</div>
										<RelativeTime date={job.last_run_at} />
									</li>
								))}
							</ul>
						) : (
							<EmptyState title="暂无执行记录" description="还没有任何 Cron 任务被执行过" />
						)}
					</CardContent>
				</Card>

				<Card>
					<CardHeader>
						<CardTitle>最近更新的设置</CardTitle>
					</CardHeader>
					<CardContent>
						{recentSettings.length > 0 ? (
							<ul className="space-y-3">
								{recentSettings.map((setting) => (
									<li key={setting.key} className="flex items-center justify-between text-sm">
										<div className="min-w-0">
											<p className="font-medium">{setting.key}</p>
											<p className="truncate text-xs text-muted-foreground">
												{setting.value}
											</p>
										</div>
										<RelativeTime date={setting.updated_at} />
									</li>
								))}
							</ul>
						) : (
							<EmptyState title="暂无设置更新" description="还没有任何设置项被更新过" />
						)}
					</CardContent>
				</Card>
			</div>
		</div>
	);
}
```

- [ ] **Step 2: Update App.tsx**

修改路由：

```tsx
import AppLayout from "@/components/layout";
import { Toaster } from "@/components/ui/toaster";
import { useInitTheme } from "@/hooks/use-theme";
import { lazy } from "react";
import { Route, Routes } from "react-router-dom";

const OverviewPage = lazy(() => import("./pages/overview"));
const CronJobsPage = lazy(() => import("./pages/cron-jobs"));
const SettingsPage = lazy(() => import("./pages/settings"));

function App() {
	useInitTheme();

	return (
		<>
			<Routes>
				<Route element={<AppLayout />}>
					<Route path="/" element={<OverviewPage />} />
					<Route path="/cron-jobs" element={<CronJobsPage />} />
					<Route path="/settings" element={<SettingsPage />} />
				</Route>
			</Routes>
			<Toaster />
		</>
	);
}

export default App;
```

- [ ] **Step 3: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add web/src/pages/overview.tsx web/src/App.tsx
git commit -m "feat(web): add Overview dashboard page"
```

---

## Task 16: Refactor CronJobTable

**Files:**
- Modify: `web/src/components/cron-jobs/CronJobTable.tsx`

**Context:** 表格卡片化、DropdownMenu 操作列、StatusBadge、RelativeTime、EmptyState。

- [ ] **Step 1: Replace CronJobTable.tsx**

完整替换为：

```tsx
import { EmptyState } from "@/components/empty-state";
import { RelativeTime } from "@/components/relative-time";
import { StatusBadge } from "@/components/status-badge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { TableSkeleton } from "@/components/table-skeleton";
import { type CronJob, useRunCronJob, useUpdateCronJob } from "@/hooks/use-cron-jobs";
import { useToast } from "@/hooks/use-toast";
import { MoreHorizontal, Pencil, Play, Trash2 } from "lucide-react";
import { useMemo } from "react";

interface CronJobTableProps {
	jobs: CronJob[] | undefined;
	isLoading: boolean;
	searchQuery?: string;
	groupFilter?: string;
	enabledFilter?: "all" | "enabled" | "disabled";
	onEdit: (job: CronJob) => void;
	onDelete: (name: string) => void;
}

function formatDate(dateStr: string) {
	if (!dateStr) return "—";
	const ts = new Date(dateStr).getTime();
	if (Number.isNaN(ts) || ts <= 0) return "—";
	return new Date(dateStr).toLocaleString("zh-CN");
}

export function CronJobTable({
	jobs,
	isLoading,
	searchQuery = "",
	groupFilter = "all",
	enabledFilter = "all",
	onEdit,
	onDelete,
}: CronJobTableProps) {
	const { toast } = useToast();
	const updateCronJob = useUpdateCronJob();
	const runCronJob = useRunCronJob();

	const filteredJobs = useMemo(() => {
		let list = jobs ?? [];
		if (searchQuery.trim()) {
			const q = searchQuery.toLowerCase();
			list = list.filter(
				(j) =>
					j.name.toLowerCase().includes(q) ||
					j.title.toLowerCase().includes(q) ||
					j.expression.toLowerCase().includes(q),
			);
		}
		if (groupFilter !== "all") {
			list = list.filter((j) => (j.group || "默认") === groupFilter);
		}
		if (enabledFilter !== "all") {
			list = list.filter((j) => (enabledFilter === "enabled" ? j.enabled : !j.enabled));
		}
		return list;
	}, [jobs, searchQuery, groupFilter, enabledFilter]);

	const groupedJobs = useMemo(() => {
		const map = new Map<string, CronJob[]>();
		for (const job of filteredJobs) {
			const key = job.group || "默认";
			const list = map.get(key) ?? [];
			list.push(job);
			map.set(key, list);
		}
		return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
	}, [filteredJobs]);

	if (isLoading) {
		return <TableSkeleton columns={7} rows={5} />;
	}

	return (
		<div className="flex-1 overflow-auto min-h-0 space-y-6">
			{groupedJobs.map(([group, groupJobs]) => (
				<div key={group} className="rounded-xl border bg-card shadow-sm">
					<div className="flex items-center justify-between border-b bg-indigo-50/50 px-5 py-3 dark:bg-indigo-950/20">
						<h2 className="font-semibold">{group}</h2>
						<Badge variant="outline">{groupJobs.length}</Badge>
					</div>
					<Table>
						<TableHeader>
							<TableRow className="hover:bg-transparent">
								<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">名称</TableHead>
								<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">标题</TableHead>
								<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">表达式</TableHead>
								<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">状态</TableHead>
								<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">上次执行</TableHead>
								<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">下次执行</TableHead>
								<TableHead className="text-right text-xs font-medium uppercase tracking-wider text-muted-foreground">操作</TableHead>
							</TableRow>
						</TableHeader>
						<TableBody>
							{groupJobs.map((job) => (
								<TableRow key={job.name} className="transition-colors hover:bg-muted/50">
									<TableCell className="font-medium">{job.name}</TableCell>
									<TableCell>{job.title}</TableCell>
									<TableCell>
										<Badge variant="outline" className="font-mono text-xs">
											{job.expression}
										</Badge>
									</TableCell>
									<TableCell>
										<div className="flex items-center gap-2">
											<Switch
												checked={job.enabled}
												disabled={updateCronJob.isPending}
												aria-label={`切换任务 ${job.name} 状态`}
												onCheckedChange={() =>
													updateCronJob.mutate(
														{ name: job.name, enabled: !job.enabled },
														{
															onSuccess: () => toast({ title: "操作成功" }),
															onError: (error) =>
																toast({
																	title: "操作失败",
																	description: error.message,
																	variant: "destructive",
																}),
														},
													)
												}
											/>
											<StatusBadge status={job.enabled ? "enabled" : "disabled"} />
										</div>
									</TableCell>
									<TableCell>
										{job.last_run_at && new Date(job.last_run_at).getTime() > 0 ? (
											<RelativeTime date={job.last_run_at} fallback="等待执行" />
										) : (
											<span className="text-muted-foreground">等待执行</span>
										)}
									</TableCell>
									<TableCell>{formatDate(job.next_run_at)}</TableCell>
									<TableCell className="text-right">
										<DropdownMenu>
											<DropdownMenuTrigger asChild>
												<Button variant="ghost" size="icon" aria-label="操作">
													<MoreHorizontal className="size-4" />
												</Button>
											</DropdownMenuTrigger>
											<DropdownMenuContent align="end">
												<DropdownMenuItem
													onClick={() =>
														runCronJob.mutate(job.name, {
															onSuccess: () => toast({ title: "任务已触发执行" }),
															onError: (error) =>
																toast({
																	title: "执行失败",
																	description: error.message,
																	variant: "destructive",
																}),
														})
													}
												>
													<Play className="mr-2 size-4" />
													立即执行
												</DropdownMenuItem>
												<DropdownMenuItem onClick={() => onEdit(job)}>
													<Pencil className="mr-2 size-4" />
													编辑
												</DropdownMenuItem>
												<DropdownMenuItem
													className="text-destructive focus:text-destructive"
													onClick={() => onDelete(job.name)}
												>
													<Trash2 className="mr-2 size-4" />
													删除
												</DropdownMenuItem>
											</DropdownMenuContent>
										</DropdownMenu>
									</TableCell>
								</TableRow>
							))}
						</TableBody>
					</Table>
				</div>
			))}
			{groupedJobs.length === 0 && (
				<EmptyState
					title="暂无定时任务"
					description="没有找到符合条件的定时任务"
				/>
			)}
		</div>
	);
}
```

- [ ] **Step 2: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/cron-jobs/CronJobTable.tsx
git commit -m "feat(web): refactor CronJobTable with cards, dropdown actions and status badges"
```

---

## Task 17: Refactor CronJobs page

**Files:**
- Modify: `web/src/pages/cron-jobs.tsx`

- [ ] **Step 1: Replace cron-jobs.tsx**

完整替换为：

```tsx
import { CronJobDeleteDialog } from "@/components/cron-jobs/CronJobDeleteDialog";
import { CronJobEditDialog } from "@/components/cron-jobs/CronJobEditDialog";
import { CronJobTable } from "@/components/cron-jobs/CronJobTable";
import { DataTableToolbar } from "@/components/data-table-toolbar";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { SearchInput } from "@/components/search-input";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import { Button } from "@/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { useCronStats } from "@/hooks/use-cron-stats";
import { type CronJob, useCronJobs } from "@/hooks/use-cron-jobs";
import { Activity, Clock, Layers, Play, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";

export default function CronJobsPage() {
	const [editingJob, setEditingJob] = useState<CronJob | null>(null);
	const [deletingJobName, setDeletingJobName] = useState<string | null>(null);
	const [searchQuery, setSearchQuery] = useState("");
	const [groupFilter, setGroupFilter] = useState("all");
	const [enabledFilter, setEnabledFilter] = useState<"all" | "enabled" | "disabled">("all");

	const { data: jobs, isLoading, refetch } = useCronJobs();
	const stats = useCronStats();

	const groups = useMemo(() => {
		const set = new Set((jobs ?? []).map((j) => j.group || "默认"));
		return Array.from(set).sort();
	}, [jobs]);

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<StatsCardsSkeleton count={4} />
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				icon={Clock}
				title="定时任务"
				description="管理系统定时任务，支持启用/禁用、立即执行与编辑"
			>
				<Button variant="outline" size="sm" onClick={() => refetch()}>
					<RefreshCw className="mr-2 size-4" />
					刷新
				</Button>
			</PageHeader>

			<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
				<StatsCard icon={Clock} label="总任务数" value={stats.total} subLabel="全部任务" />
				<StatsCard icon={Play} label="已启用" value={stats.enabled} subLabel="运行中" />
				<StatsCard icon={Layers} label="分组数" value={stats.groups} subLabel="任务分组" />
				<StatsCard
					icon={Activity}
					label="最近执行"
					value={stats.lastRun ? new Date(stats.lastRun.last_run_at).toLocaleString("zh-CN") : "—"}
					subLabel="最近一次执行"
				/>
			</div>

			<DataTableToolbar>
				<SearchInput
					value={searchQuery}
					onChange={setSearchQuery}
					placeholder="搜索名称、标题、表达式..."
				/>
				<div className="flex items-center gap-2">
					<Select value={groupFilter} onValueChange={setGroupFilter}>
						<SelectTrigger className="w-[160px]">
							<SelectValue placeholder="全部分组" />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">全部分组</SelectItem>
							{groups.map((group) => (
								<SelectItem key={group} value={group}>
									{group}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
					<Select
						value={enabledFilter}
						onValueChange={(v) => setEnabledFilter(v as typeof enabledFilter)}
					>
						<SelectTrigger className="w-[140px]">
							<SelectValue placeholder="全部状态" />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">全部状态</SelectItem>
							<SelectItem value="enabled">已启用</SelectItem>
							<SelectItem value="disabled">已禁用</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</DataTableToolbar>

			<CronJobTable
				jobs={jobs}
				isLoading={isLoading}
				searchQuery={searchQuery}
				groupFilter={groupFilter}
				enabledFilter={enabledFilter}
				onEdit={setEditingJob}
				onDelete={setDeletingJobName}
			/>

			<CronJobEditDialog
				job={editingJob}
				open={!!editingJob}
				onOpenChange={(open) => !open && setEditingJob(null)}
			/>

			<CronJobDeleteDialog
				jobName={deletingJobName}
				open={!!deletingJobName}
				onOpenChange={(open) => !open && setDeletingJobName(null)}
			/>
		</div>
	);
}
```

- [ ] **Step 2: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/pages/cron-jobs.tsx
git commit -m "feat(web): refactor CronJobs page with header, stats and toolbar"
```

---

## Task 18: Refactor SettingsTable

**Files:**
- Modify: `web/src/components/settings/SettingsTable.tsx`

- [ ] **Step 1: Replace SettingsTable.tsx**

完整替换为：

```tsx
import { EmptyState } from "@/components/empty-state";
import { RelativeTime } from "@/components/relative-time";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { TableSkeleton } from "@/components/table-skeleton";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import type { Setting } from "@/hooks/use-settings";
import { Pencil } from "lucide-react";

interface SettingsTableProps {
	settings: Setting[] | undefined;
	isLoading: boolean;
	onEdit: (setting: Setting) => void;
}

function getTypeBadgeVariant(type: string) {
	switch (type) {
		case "String":
			return "bg-indigo-100 text-indigo-700 hover:bg-indigo-100 dark:bg-indigo-900/30 dark:text-indigo-400";
		case "Int":
			return "bg-blue-100 text-blue-700 hover:bg-blue-100 dark:bg-blue-900/30 dark:text-blue-400";
		case "Float":
			return "bg-amber-100 text-amber-700 hover:bg-amber-100 dark:bg-amber-900/30 dark:text-amber-400";
		case "Bool":
			return "bg-emerald-100 text-emerald-700 hover:bg-emerald-100 dark:bg-emerald-900/30 dark:text-emerald-400";
		default:
			return "bg-slate-100 text-slate-700 hover:bg-slate-100 dark:bg-slate-800 dark:text-slate-400";
	}
}

export function SettingsTable({ settings, isLoading, onEdit }: SettingsTableProps) {
	if (isLoading) {
		return <TableSkeleton columns={5} rows={5} />;
	}

	return (
		<div className="rounded-xl border bg-card shadow-sm">
			<Table>
				<TableHeader>
					<TableRow className="hover:bg-transparent">
						<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">键</TableHead>
						<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">值</TableHead>
						<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">类型</TableHead>
						<TableHead className="text-xs font-medium uppercase tracking-wider text-muted-foreground">更新时间</TableHead>
						<TableHead className="text-right text-xs font-medium uppercase tracking-wider text-muted-foreground">操作</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{settings?.map((setting) => (
						<TableRow key={setting.key} className="transition-colors hover:bg-muted/50">
							<TableCell className="font-medium">{setting.key}</TableCell>
							<TableCell className="max-w-xs">
								<TooltipProvider>
									<Tooltip>
										<TooltipTrigger asChild>
											<span className="block truncate">{setting.value}</span>
										</TooltipTrigger>
										<TooltipContent>
											<p className="max-w-md break-all">{setting.value}</p>
										</TooltipContent>
									</Tooltip>
								</TooltipProvider>
							</TableCell>
							<TableCell>
								<Badge className={getTypeBadgeVariant(setting.type)}>{setting.type}</Badge>
							</TableCell>
							<TableCell>
								<RelativeTime date={setting.updated_at} />
							</TableCell>
							<TableCell className="text-right">
								<Button variant="ghost" size="sm" onClick={() => onEdit(setting)}>
									<Pencil className="mr-2 size-4" />
									编辑
								</Button>
							</TableCell>
						</TableRow>
					))}
					{(!settings || settings.length === 0) && (
						<TableRow>
							<TableCell colSpan={5} className="py-0">
								<EmptyState title="暂无设置项" description="没有找到任何系统配置项" />
							</TableCell>
						</TableRow>
					)}
				</TableBody>
			</Table>
		</div>
	);
}
```

- [ ] **Step 2: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/settings/SettingsTable.tsx
git commit -m "feat(web): refactor SettingsTable with cards, colored badges and relative time"
```

---

## Task 19: Refactor Settings page

**Files:**
- Modify: `web/src/pages/settings.tsx`

- [ ] **Step 1: Replace settings.tsx**

完整替换为：

```tsx
import { DataTableToolbar } from "@/components/data-table-toolbar";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { SearchInput } from "@/components/search-input";
import { SettingEditDialog } from "@/components/settings/SettingEditDialog";
import { SettingsTable } from "@/components/settings/SettingsTable";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { type Setting, useSettings } from "@/hooks/use-settings";
import { Settings } from "lucide-react";
import { useMemo, useState } from "react";

const typeOptions = ["String", "Int", "Float", "Bool"];

export default function SettingsPage() {
	const [editingSetting, setEditingSetting] = useState<Setting | null>(null);
	const [searchQuery, setSearchQuery] = useState("");
	const [typeFilter, setTypeFilter] = useState("all");

	const { data: settings, isLoading } = useSettings();

	const filteredSettings = useMemo(() => {
		let list = settings ?? [];
		if (searchQuery.trim()) {
			const q = searchQuery.toLowerCase();
			list = list.filter(
				(s) => s.key.toLowerCase().includes(q) || s.value.toLowerCase().includes(q),
			);
		}
		if (typeFilter !== "all") {
			list = list.filter((s) => s.type === typeFilter);
		}
		return list;
	}, [settings, searchQuery, typeFilter]);

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<SettingsTable settings={undefined} isLoading={true} onEdit={() => {}} />
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				icon={Settings}
				title="系统设置"
				description="管理应用配置项"
			/>

			<DataTableToolbar>
				<SearchInput
					value={searchQuery}
					onChange={setSearchQuery}
					placeholder="搜索键或值..."
				/>
				<Select value={typeFilter} onValueChange={setTypeFilter}>
					<SelectTrigger className="w-[160px]">
						<SelectValue placeholder="全部类型" />
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="all">全部类型</SelectItem>
						{typeOptions.map((type) => (
							<SelectItem key={type} value={type}>
								{type}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			</DataTableToolbar>

			<SettingsTable
				settings={filteredSettings}
				isLoading={isLoading}
				onEdit={setEditingSetting}
			/>

			<SettingEditDialog
				setting={editingSetting}
				open={!!editingSetting}
				onOpenChange={(open) => !open && setEditingSetting(null)}
			/>
		</div>
	);
}
```

- [ ] **Step 2: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/pages/settings.tsx
git commit -m "feat(web): refactor Settings page with header, search and type filter"
```

---

## Task 20: Polish Dialogs and remove unused PageSkeleton

**Files:**
- Modify: `web/src/components/cron-jobs/CronJobEditDialog.tsx`
- Modify: `web/src/components/cron-jobs/CronJobDeleteDialog.tsx`
- Modify: `web/src/components/settings/SettingEditDialog.tsx`
- Delete: `web/src/components/page-skeleton.tsx`

**Context:** 统一 Dialog 内容宽度、Header/Footer 间距；`page-skeleton.tsx` 被新的区块骨架替代。

- [ ] **Step 1: Update CronJobEditDialog**

修改三处：

```tsx
<DialogContent className="sm:max-w-[500px]">
```

```tsx
<DialogHeader className="space-y-3">
```

```tsx
<DialogFooter className="gap-2">
```

- [ ] **Step 2: Update CronJobDeleteDialog**

修改三处：

```tsx
<DialogContent className="sm:max-w-[400px]">
```

```tsx
<DialogHeader className="space-y-3">
```

```tsx
<DialogFooter className="gap-2">
```

- [ ] **Step 3: Update SettingEditDialog**

修改三处：

```tsx
<DialogContent className="sm:max-w-[500px]">
```

```tsx
<DialogHeader className="space-y-3">
```

```tsx
<DialogFooter className="gap-2">
```

- [ ] **Step 4: Delete page-skeleton.tsx**

```bash
rm /Users/ijkzen/Projects/RUST-Project/rs-template/web/src/components/page-skeleton.tsx
```

- [ ] **Step 5: Verify build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/cron-jobs/CronJobEditDialog.tsx web/src/components/cron-jobs/CronJobDeleteDialog.tsx web/src/components/settings/SettingEditDialog.tsx
git rm web/src/components/page-skeleton.tsx
git commit -m "feat(web): polish dialog styles and remove old page skeleton"
```

---

## Task 21: Run all tests and lint

**Files:**
- All modified files

- [ ] **Step 1: Run frontend tests**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm test -- --run
```

Expected: All tests pass.

- [ ] **Step 2: Run lint**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm lint
```

Expected: No errors. If Biome reports formatting issues, run format.

- [ ] **Step 3: Run format**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm format
```

- [ ] **Step 4: Run production build**

```bash
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm build
```

Expected: Build succeeds.

- [ ] **Step 5: Commit fixes**

```bash
git add -A
git commit -m "chore(web): fix lint and format after redesign"
```

---

## Task 22: Manual visual verification

**Context：** 自动化测试无法覆盖视觉细节，需要人工检查。

- [ ] **Step 1: Start dev server and backend**

```bash
# Terminal 1
cargo run

# Terminal 2
cd /Users/ijkzen/Projects/RUST-Project/rs-template/web
pnpm dev
```

- [ ] **Step 2: Verify pages**

- [ ] Overview 页面显示 4 个 StatsCard 和两栏最近列表。
- [ ] Cron Jobs 页面有 PageHeader、Stats Cards、搜索/筛选工具栏、新表格、DropdownMenu 操作。
- [ ] Settings 页面有 PageHeader、搜索/类型筛选、新表格、彩色类型 badge。
- [ ] 切换 Light / Dark 模式无明显问题。
- [ ] 移动端 sidebar 可正常收起，表格可横向滚动。
- [ ] 所有现有功能（启用/禁用、立即执行、编辑、删除）正常工作。

- [ ] **Step 3: Document any issues**

如有视觉或交互问题，记录并修复后再提交。

---

## Plan Self-Review

### Spec coverage

| Spec Section | Implementing Task |
|---|---|
| 设计系统（颜色、字体、间距） | Task 5, 6 |
| Sidebar/Header 重构 | Task 14 |
| Overview 页面 | Task 15 |
| Cron Jobs 页面重构 | Task 16, 17 |
| Settings 页面重构 | Task 18, 19 |
| 共享组件 | Task 7-13 |
| Dialog 样式统一 | Task 20 |
| 测试 | Tasks 7-12, Task 21 |
| 验收标准 | Task 21, 22 |

### Placeholder scan

- 无 TBD / TODO。
- 所有组件代码完整。
- 所有命令和期望输出明确。

### Type consistency

- `StatusBadgeProps.status` 在组件和测试中一致。
- `SearchInputProps` 在组件和测试中一致。
- `CronJobTableProps` 扩展了筛选属性，与 `CronJobsPage` 调用一致。
- `Setting` 与 `CronJob` 类型复用现有 hooks，未改动接口。

### Dependency note

- 新增 `@radix-ui/react-dropdown-menu` 已在 Task 1 安装。
- 新增 `card.tsx` / `dropdown-menu.tsx` shadcn 组件文件在 Task 3 / 4 创建。

### Risk note

- 已移除 `import.meta.env` 使用，避免 Vitest/TypeScript 类型问题；SidebarFooter 仅显示静态版本号。
