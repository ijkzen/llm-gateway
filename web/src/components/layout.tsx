import { SkipToMain } from "@/components/skip-to-main";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
import { useLogout, useMe } from "@/hooks/use-auth";
import { PAGES } from "@/lib/pages";
import { useQueryClient } from "@tanstack/react-query";
import { ChevronUp, LogOut, RefreshCw, Settings } from "lucide-react";
import { Suspense } from "react";
import { Link, Outlet, useLocation, useNavigate } from "react-router-dom";

export default function AppLayout() {
	const location = useLocation();
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const { data: me } = useMe();
	const logout = useLogout();

	const handleLogout = () => {
		logout.mutate(undefined, {
			onSettled: () => {
				queryClient.clear();
				navigate("/login", { replace: true });
			},
		});
	};

	return (
		<SidebarProvider>
			<SkipToMain />
			<Sidebar variant="floating" className="sidebar-surface z-30">
				<SidebarHeader>
					<SidebarMenu>
						<SidebarMenuItem>
							<SidebarMenuButton size="lg" asChild>
								<Link to="/">
									<div className="flex aspect-square size-9 items-center justify-center rounded-xl bg-foreground text-background shadow-[inset_0_1px_0_rgba(255,255,255,0.15),0_4px_10px_rgba(15,23,42,0.18)] dark:bg-primary dark:text-primary-foreground">
										<Settings className="size-4" />
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
					<SidebarMenu>
						<SidebarMenuItem>
							<DropdownMenu>
								<DropdownMenuTrigger asChild>
									<SidebarMenuButton size="lg" aria-label="用户菜单">
										<div className="flex aspect-square size-8 items-center justify-center rounded-full bg-foreground/10 text-sm font-semibold uppercase text-foreground">
											{(me?.username ?? "?").slice(0, 1)}
										</div>
										<div className="flex min-w-0 flex-col leading-none">
											<span className="truncate font-medium">{me?.username ?? "..."}</span>
											<span className="text-xs text-muted-foreground">已登录</span>
										</div>
										<ChevronUp className="ml-auto size-4" />
									</SidebarMenuButton>
								</DropdownMenuTrigger>
								<DropdownMenuContent side="top" align="start" className="min-w-[180px]">
									<DropdownMenuLabel className="truncate">{me?.username}</DropdownMenuLabel>
									<DropdownMenuSeparator />
									<DropdownMenuItem onClick={() => navigate("/settings")}>
										<Settings className="size-4" />
										设置
									</DropdownMenuItem>
									<DropdownMenuItem variant="destructive" onClick={handleLogout}>
										<LogOut className="size-4" />
										退出登录
									</DropdownMenuItem>
								</DropdownMenuContent>
							</DropdownMenu>
						</SidebarMenuItem>
					</SidebarMenu>
					<div className="px-4 py-2 text-xs text-muted-foreground">
						<div>RS Template v0.1.0</div>
					</div>
				</SidebarFooter>
			</Sidebar>
			<SidebarInset className="overflow-hidden">
				{/* 吸顶样式由 CSS scroll-state 查询驱动，见 sticky-header.css 的 .app-header */}
				<header className="app-header sticky top-0 z-10 shrink-0">
					<div className="app-header-inner flex h-14 items-center gap-4 px-6">
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
