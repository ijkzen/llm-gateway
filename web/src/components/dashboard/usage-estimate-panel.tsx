import type { UsageEstimate } from "@/hooks/use-usage-estimate";
import { formatTokenCount } from "@/lib/utils";
import { AlertTriangle, Sparkles } from "lucide-react";

/** 预估窗口的中文名（weekly / monthly / 其他）。 */
function windowLabel(window: string): string {
	if (window === "weekly") return "本周";
	if (window === "monthly") return "本月";
	return "订阅周期";
}

/**
 * 订阅周期 Token 总量预估展示。
 * - estimatable：显示预估总量（标注周/月窗口）+ 已用 token + 覆盖天数；
 * - 否则显示「数据覆盖不足，无法预估」提示。
 */
export function UsageEstimatePanel({
	estimate,
}: {
	estimate: UsageEstimate | undefined;
}) {
	if (!estimate) {
		return null;
	}
	if (!estimate.estimatable || estimate.estimatedTotalTokens === null) {
		return (
			<div className="mt-3 flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm">
				<AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" />
				<div>
					<p className="font-medium text-amber-700 dark:text-amber-400">数据覆盖不足，无法预估</p>
					<p className="mt-0.5 text-xs text-muted-foreground">
						{windowLabel(estimate.window)}订阅窗口共 {estimate.totalDays} 天，仅有{" "}
						{estimate.coveredDays} 天有请求数据，无法准确预估订阅周期内可用 Token 总量。
					</p>
				</div>
			</div>
		);
	}
	return (
		<div className="mt-3 flex items-start gap-2 rounded-lg border bg-primary/5 p-3 text-sm">
			<Sparkles className="mt-0.5 size-4 shrink-0 text-primary" />
			<div>
				<p className="font-medium text-foreground">
					预估{windowLabel(estimate.window)}可用 Token：
					<span className="ml-1 font-mono tabular-nums">
						{formatTokenCount(estimate.estimatedTotalTokens ?? 0)}
					</span>
				</p>
				<p className="mt-0.5 text-xs text-muted-foreground">
					当前窗口已用 {formatTokenCount(estimate.usedTokens)}（覆盖 {estimate.coveredDays}/
					{estimate.totalDays} 天）
				</p>
			</div>
		</div>
	);
}
