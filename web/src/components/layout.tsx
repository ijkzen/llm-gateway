import { SkipToMain } from "@/components/skip-to-main";
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
import { PAGES } from "@/lib/pages";
import { useQueryClient } from "@tanstack/react-query";
import { RefreshCw, Settings } from "lucide-react";
import { Suspense } from "react";
import { Link, Outlet, useLocation } from "react-router-dom";

export default function AppLayout() {
	const location = useLocation();
	const queryClient = useQueryClient();

	return (
		<SidebarProvider>
			<SkipToMain />
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
								{PAGES.map((page) => (
									<SidebarMenuItem key={page.path}>
										<SidebarMenuButton asChild isActive={location.pathname === page.path}>
											<Link to={page.path}>
												<page.icon />
												<span>{page.title}</span>
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
			<SidebarInset className="[overflow-anchor:none]">
				{/* 吸顶样式由 CSS scroll-state 查询驱动，见 index.css 的 .app-header */}
				<header className="app-header sticky top-0 z-10 shrink-0 border-b">
					<div className="app-header-inner flex h-16 items-center gap-4 bg-card px-6">
						<SidebarTrigger className="-ml-2" aria-label="切换侧边栏" />
						<Separator orientation="vertical" className="h-6" />
						<div className="flex flex-1 items-center justify-end gap-2">
							<Button
								variant="outline"
								size="icon"
								title="刷新"
								aria-label="刷新"
								onClick={() => queryClient.invalidateQueries()}
							>
								<RefreshCw className="size-4" />
							</Button>
							<ThemeToggle />
						</div>
					</div>
				</header>
				<div id="content" className="mx-auto flex w-full max-w-7xl flex-1 flex-col gap-6 p-6">
					<Suspense
						fallback={
							<div
								className="flex flex-1 items-center justify-center text-muted-foreground"
								aria-busy="true"
								aria-live="polite"
							>
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
