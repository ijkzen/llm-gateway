import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { type Query, useIsFetching, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

/** 页面刷新排除的布局级键：登录态清了会触发路由守卫全屏验证（失败即踢回登录页），版本号重取无意义。 */
const LAYOUT_KEYS = ["auth", "health"];

/** 刷新范围与按钮忙碌态的同一判定：两个调用点必须一致，故共用此谓词。 */
function isRefreshableQuery(query: Query): boolean {
	return !LAYOUT_KEYS.includes(String(query.queryKey[0]));
}

/**
 * 顶栏页面刷新按钮：清空除布局级键外的全部查询缓存并重新取数当前页面。
 * 用 resetQueries（先清数据再立即重取 active，忽略 staleTime）而非
 * removeQueries（active 不自动重取）或 invalidateQueries（不删数据）。
 */
export function PageRefreshButton() {
	const { t } = useTranslation();
	const queryClient = useQueryClient();
	const isRefreshing = useIsFetching({ predicate: isRefreshableQuery }) > 0;

	const refresh = () => {
		queryClient.resetQueries({ predicate: isRefreshableQuery });
	};

	return (
		<Button
			variant="outline"
			size="icon"
			title={t("common.refresh")}
			aria-label={t("common.refresh")}
			disabled={isRefreshing}
			onClick={refresh}
		>
			<RefreshCw className={cn("size-4", isRefreshing && "animate-spin")} />
		</Button>
	);
}
