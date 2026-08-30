import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { type UsageWindow, useProviderUsage } from "@/hooks/use-provider-usage";
import type { UsageEstimate } from "@/hooks/use-usage-estimate";
import { cn, formatTokenCount } from "@/lib/utils";
import { AlertCircle, RefreshCw } from "lucide-react";
import { useState } from "react";

const WINDOW_LABELS: Record<UsageWindow["window"], string> = {
	five_hour: "5 小时",
	weekly: "本周",
	monthly: "本月",
};

/** 预估的月 Token 总量：周窗口 ×4 折算为月，月窗口直接用。 */
function monthlyEstimateTokens(estimate: UsageEstimate): number | null {
	if (!estimate.estimatable || estimate.estimatedTotalTokens == null) {
		return null;
	}
	if (estimate.window === "weekly") {
		return estimate.estimatedTotalTokens * 4;
	}
	return estimate.estimatedTotalTokens;
}

/** provider.extra 中是否开启了用量查询（决定详情页是否渲染本卡片）。 */
export function usageEnabled(extra: string): boolean {
	try {
		return (JSON.parse(extra) as Record<string, unknown>).usage === true;
	} catch {
		return false;
	}
}

/** 剩余百分比 → 进度条颜色：充足绿 / 紧张黄 / 告急红。 */
function barColor(remainingPercent: number): string {
	if (remainingPercent > 50) return "bg-emerald-500";
	if (remainingPercent > 20) return "bg-amber-500";
	return "bg-red-500";
}

function formatReset(resetsAt?: string): string | null {
	if (!resetsAt) return null;
	const ts = new Date(resetsAt).getTime();
	if (Number.isNaN(ts) || ts <= 0) return null;
	const d = new Date(ts);
	return `${d.getMonth() + 1}月${d.getDate()}日 ${String(d.getHours()).padStart(2, "0")}:${String(
		d.getMinutes(),
	).padStart(2, "0")} 重置`;
}

function formatAmount(amount: number): string {
	return amount.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}

function WindowRow({ window }: { window: UsageWindow }) {
	// 后端已在接口出口按 remaining_percent_value() 推导并取整（round2），
	// 前端直接使用，不再自行推导/取整。
	const remaining = window.remainingPercent;
	const reset = formatReset(window.resetsAt);
	return (
		<div className="space-y-1.5">
			<div className="flex items-baseline justify-between gap-2 text-sm">
				<span className="text-muted-foreground">{WINDOW_LABELS[window.window]}</span>
				<span className="font-medium tabular-nums">
					{remaining !== undefined ? `剩余 ${remaining}%` : "—"}
				</span>
			</div>
			{remaining !== undefined && (
				<div className="h-2 w-full overflow-hidden rounded-full bg-muted">
					<div
						className={cn("h-full rounded-full transition-all", barColor(remaining))}
						style={{ width: `${Math.min(100, Math.max(0, remaining))}%` }}
					/>
				</div>
			)}
			<div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
				<span>
					{window.used !== undefined && window.limit !== undefined
						? `已用 ${formatAmount(window.used)} / ${formatAmount(window.limit)}${window.unit ? ` ${window.unit}` : ""}`
						: ""}
				</span>
				<span>{reset ?? ""}</span>
			</div>
		</div>
	);
}

export function ProviderUsageCard({
	providerId,
	estimate,
}: {
	providerId: number;
	/** 订阅周期 Token 预估（可预估时在右下角展示月 Token 总量）。 */
	estimate?: UsageEstimate | undefined;
}) {
	const [refreshToken, setRefreshToken] = useState(0);
	const { data, isLoading, isFetching, error } = useProviderUsage(providerId, refreshToken);

	const refresh = () => setRefreshToken((t) => t + 1);

	const availableWindows = data?.windows?.filter((w) => w.available) ?? [];
	const monthlyTokens = estimate ? monthlyEstimateTokens(estimate) : null;

	return (
		<div className="rounded-lg border bg-card/50 p-4">
			<div className="mb-3 flex items-center justify-between gap-2">
				<div className="flex items-center gap-2">
					<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
						用量信息
					</p>
					{data?.plan && <Badge variant="secondary">{data.plan}</Badge>}
				</div>
				<Button
					type="button"
					variant="ghost"
					size="icon"
					className="size-7"
					aria-label="刷新用量"
					disabled={isFetching}
					onClick={refresh}
				>
					<RefreshCw className={cn("size-4", isFetching && "animate-spin")} />
				</Button>
			</div>

			{isLoading ? (
				<div className="space-y-3">
					<Skeleton className="h-4 w-3/4" />
					<Skeleton className="h-2 w-full" />
					<Skeleton className="h-4 w-1/2" />
				</div>
			) : error ? (
				<div className="flex items-center justify-between gap-2">
					<p className="flex items-center gap-1.5 text-sm text-destructive">
						<AlertCircle className="size-4 shrink-0" />
						<span className="line-clamp-2">{error.message}</span>
					</p>
					<Button type="button" variant="outline" size="sm" onClick={refresh}>
						重试
					</Button>
				</div>
			) : data?.kind === "balance" ? (
				<div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
					{data.balances?.map((item) => (
						<div key={item.label}>
							<p className="text-xs text-muted-foreground">{item.label}</p>
							<p className="mt-0.5 text-lg font-semibold tabular-nums">
								{formatAmount(item.amount)}
								{item.currency && (
									<span className="ml-1 text-xs font-normal text-muted-foreground">
										{item.currency}
									</span>
								)}
							</p>
						</div>
					))}
				</div>
			) : availableWindows.length > 0 ? (
				<div className="space-y-4">
					{availableWindows.map((w) => (
						<WindowRow key={w.window} window={w} />
					))}
				</div>
			) : (
				<p className="text-sm text-muted-foreground">暂无用量数据</p>
			)}

			{/* 底部：右下角展示预估月 Token 总量（无法预估时留空）。 */}
			<div className="mt-3 flex items-end justify-between gap-2">
				{data && (
					<p className="text-xs text-muted-foreground">
						更新于 {new Date(data.fetchedAt).toLocaleTimeString("zh-CN")}
					</p>
				)}
				{monthlyTokens !== null && (
					<p className="text-right text-xs text-muted-foreground">
						预估月 Token：
						<span className="font-mono font-medium tabular-nums">
							{formatTokenCount(monthlyTokens)}
						</span>
					</p>
				)}
			</div>
		</div>
	);
}
