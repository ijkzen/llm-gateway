import { MidEllipsis } from "@/components/mid-ellipsis";
import { useProviderModelDetail } from "@/hooks/use-provider-models";
import { ChevronRight } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Link, useLocation } from "react-router-dom";

/** 数据面板详情页在结构归属树中的位置（解析用；返回 null 表示非数据面板详情页）。 */
export type BreadcrumbRoute =
	| { kind: "provider"; providerId: number }
	| { kind: "virtualModel"; virtualModelId: number }
	| { kind: "apiKey"; apiKeyId: number }
	| { kind: "model"; modelId: number };

/** 把 pathname 解析为数据面板详情页路由（非数据面板页返回 null）。 */
export function parseBreadcrumbRoute(pathname: string): BreadcrumbRoute | null {
	let m = pathname.match(/^\/providers\/(\d+)\/overview$/);
	if (m) {
		return { kind: "provider", providerId: Number(m[1]) };
	}
	m = pathname.match(/^\/virtual-models\/(\d+)\/overview$/);
	if (m) {
		return { kind: "virtualModel", virtualModelId: Number(m[1]) };
	}
	m = pathname.match(/^\/api-keys\/(\d+)\/overview$/);
	if (m) {
		return { kind: "apiKey", apiKeyId: Number(m[1]) };
	}
	m = pathname.match(/^\/models\/(\d+)\/overview$/);
	if (m) {
		return { kind: "model", modelId: Number(m[1]) };
	}
	return null;
}

interface Crumb {
	/** 展示文本。 */
	label: string;
	/** 链接目标；缺省 = 当前层（非链接灰显）。 */
	to?: string;
}

/**
 * 结构归属链面包屑（不含当前页）。按路由解析当前实体在归属树中的位置：
 * 供应商/虚拟模型/API Key 面板 = 首页 › 所属列表；模型面板 = 首页 › 供应商列表 ›
 * {所属供应商名}（中间层指向该供应商数据面板，供应商名经模型 detail 解析）。
 * 非数据面板详情页不渲染。模型路由的 detail 与页面同 key，缓存命中不重复请求。
 */
export function Breadcrumbs({ className }: { className?: string }) {
	const { t } = useTranslation();
	const location = useLocation();
	const route = parseBreadcrumbRoute(location.pathname);

	// 仅模型路由需要取供应商名做中间层；其它路由传入 null 禁用请求。
	const modelId = route?.kind === "model" ? route.modelId : null;
	const modelDetail = useProviderModelDetail(modelId);

	const crumbs = useMemo<Crumb[] | null>(() => {
		if (!route) {
			return null;
		}
		const root = { label: t("nav.pages.overview.title"), to: "/" };
		switch (route.kind) {
			case "provider":
				return [root, { label: t("nav.pages.providers.title"), to: "/providers" }];
			case "virtualModel":
				return [root, { label: t("nav.pages.virtualModels.title"), to: "/virtual-models" }];
			case "apiKey":
				return [root, { label: t("nav.pages.apiKeys.title"), to: "/api-keys" }];
			case "model": {
				// detail 未返回（加载中）时只展示到供应商列表层，避免用
				// 与列表标签同文案的占位；detail 就绪后自动补第三层。
				const providerCrumb: Crumb[] = modelDetail?.data
					? [
							{
								label:
									modelDetail.data.providerName ||
									t("dashboardPage.providerLabel", { id: modelDetail.data.providerId }),
								to: `/providers/${modelDetail.data.providerId}/overview`,
							},
						]
					: [];
				return [
					root,
					{ label: t("nav.pages.providers.title"), to: "/providers" },
					...providerCrumb,
				];
			}
		}
	}, [route, modelDetail?.data, t]);

	if (!crumbs) {
		return null;
	}

	return (
		<nav aria-label="breadcrumb" className={className}>
			<ol className="flex min-w-0 items-center gap-1">
				{crumbs.map((crumb, index) => {
					// 所有可见段均为祖先（当前页不渲染）；无链接目标时（detail 未返回
					// 的模型供应商占位）降级为纯文本，避免空链接。
					const node = crumb.to ? (
						<Link
							to={crumb.to}
							className="block max-w-[180px] text-xs text-muted-foreground transition-colors hover:text-foreground"
						>
							<MidEllipsis text={crumb.label} className="text-xs" />
						</Link>
					) : (
						<span className="block max-w-[180px] text-xs text-muted-foreground">
							<MidEllipsis text={crumb.label} className="text-xs" />
						</span>
					);
					return (
						<li key={`${crumb.label}-${index}`} className="flex min-w-0 items-center gap-1">
							{index > 0 && <ChevronRight className="size-3 shrink-0 text-muted-foreground/50" />}
							{node}
						</li>
					);
				})}
			</ol>
		</nav>
	);
}
