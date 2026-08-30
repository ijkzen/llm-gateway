import { RaceWindowControl, type RaceWindowState } from "@/components/race-window-control";
import { StatsCard } from "@/components/stats-card";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { Coins, DatabaseZap, Gauge, ListChecks, Timer } from "lucide-react";

/** 6 指标卡共用的指标项定义（key / 标题 / 格式化 / 图标）。 */
interface MetricsItem {
	key: "totalTokens" | "requestCount" | "ttft" | "requestTime" | "tps" | "cacheHitRate";
	label: string;
	icon: typeof Coins;
	format: (v: number) => string;
	subLabel: string;
}

export const METRICS_ITEMS: readonly MetricsItem[] = [
	{
		key: "totalTokens",
		label: "总计 Token",
		icon: Coins,
		format: formatTokenCount,
		subLabel: "输入 + 输出",
	},
	{
		key: "requestCount",
		label: "请求数",
		icon: ListChecks,
		format: (v) => v.toLocaleString(),
		subLabel: "成功请求",
	},
	{
		key: "ttft",
		label: "TTFT",
		icon: Timer,
		format: (v) => `${v.toFixed(1)} ms`,
		subLabel: "流式首 token",
	},
	{
		key: "requestTime",
		label: "平均耗时",
		icon: Timer,
		format: (v) => `${v.toFixed(1)} ms`,
		subLabel: "成功请求",
	},
	{
		key: "tps",
		label: "TPS",
		icon: Gauge,
		format: (v) => v.toFixed(2),
		subLabel: "加权均值",
	},
	{
		key: "cacheHitRate",
		label: "缓存命中率",
		icon: DatabaseZap,
		format: formatPercent,
		subLabel: "缓存 / 输入 token",
	},
];

/** 6 指标卡所需的数据切片（provider / virtual-model 两级共用）。 */
export interface MetricsData {
	totalTokens: number;
	requestCount: number;
	ttft: number;
	requestTime: number;
	tps: number;
	cacheHitRate: number;
}

interface MetricsSummaryCardProps {
	/** 6 指标数据（undefined 表示加载中）。 */
	data: MetricsData | undefined;
	isLoading: boolean;
	windowState: RaceWindowState;
	now: number;
	onWindowChange: (patch: Partial<RaceWindowState>) => void;
	subtitle: string;
	/** 卡片标题，默认「指标概览」。 */
	title?: string;
	/** 右侧额外内容（如用量卡）。 */
	extra?: React.ReactNode;
}

/** 二级页顶部 6 指标卡片：独立时间段控件 + 6 个指标项。 */
export function MetricsSummaryCard({
	data,
	isLoading,
	windowState,
	now,
	onWindowChange,
	subtitle,
	title = "指标概览",
	extra,
}: MetricsSummaryCardProps) {
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>{title}</CardTitle>
					<p className="text-xs text-muted-foreground">{subtitle}</p>
				</div>
				<RaceWindowControl state={windowState} now={now} onChange={onWindowChange} />
			</CardHeader>
			<CardContent>
				{isLoading ? (
					<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
						{[0, 1, 2, 3, 4, 5].map((i) => (
							<Skeleton key={i} className="h-24 w-full" />
						))}
					</div>
				) : data ? (
					<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
						{METRICS_ITEMS.map((item) => (
							<StatsCard
								key={item.key}
								icon={item.icon}
								label={item.label}
								value={item.format(data[item.key])}
								subLabel={item.subLabel}
							/>
						))}
					</div>
				) : (
					<div className="flex h-[160px] items-center justify-center text-xs text-muted-foreground">
						数据加载失败
					</div>
				)}
				{extra}
			</CardContent>
		</Card>
	);
}
