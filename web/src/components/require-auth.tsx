import { useMe } from "@/hooks/use-auth";
import type { ReactNode } from "react";
import { Navigate, useLocation } from "react-router-dom";

/**
 * 路由守卫：未持有有效会话时跳转到 /login，并记录来源路径供登录后回跳。
 */
export function RequireAuth({ children }: { children: ReactNode }) {
	const location = useLocation();
	const { data, isLoading, isError } = useMe();

	if (isLoading) {
		return (
			<div
				className="flex min-h-screen flex-1 items-center justify-center text-muted-foreground"
				aria-busy="true"
				aria-live="polite"
			>
				正在验证登录状态...
			</div>
		);
	}

	if (isError || !data) {
		return <Navigate to="/login" replace state={{ from: location.pathname }} />;
	}

	return children;
}
