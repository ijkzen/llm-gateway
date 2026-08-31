import { RaceWindowControl, type RaceWindowState } from "@/components/race-window-control";
import { StatsCard } from "@/components/stats-card";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { Coins, DatabaseZap, Gauge, ListChecks, Timer } from "lucide-react";
import { useTranslation } from "react-i18next";

/** 6 指标卡共用的指标项定义（key / 标题键 / 格式化 / 图标 / 副标签键）。 */
interface MetricsItem {
	key: "totalTokens" | "requestCount" | "ttft" | "requestTime" | "tps" | "cacheHitRate";
	labelKey: string;
	icon: typeof Coins;
	format: (v: number) => string;
	subLabelKey: string;
}

export const METRICS_ITEMS: readonly MetricsItem[] = [
	{
		key: "totalTokens",
		labelKey: "race.metricLabel.totalTokens",
		icon: Coins,
		format: formatTokenCount,
		subLabelKey: "overview.inputPlusOutput",
	},
	{
		key: "requestCount",
		labelKey: "race.metricLabel.requestCount",
		icon: ListChecks,
		format: (v) => v.toLocaleString(),
		subLabelKey: "overview.successRequests",
	},
	{
		key: "ttft",
		labelKey: "race.metricLabel.ttft",
		icon: Timer,
		format: (v) => `${v.toFixed(1)} ms`,
		subLabelKey: "overview.streamFirstToken",
	},
	{
		key: "requestTime",
		labelKey: "race.metricLabel.requestTime",
		icon: Timer,
		format: (v) => `${v.toFixed(1)} ms`,
		subLabelKey: "overview.successRequests",
	},
	{
		key: "tps",
		labelKey: "race.metricLabel.tps",
		icon: Gauge,
		format: (v) => v.toFixed(2),
		subLabelKey: "overview.weightedAverage",
	},
	{
		key: "cacheHitRate",
		labelKey: "race.metricLabel.cacheHitRate",
		icon: DatabaseZap,
		format: formatPercent,
		subLabelKey: "overview.cacheOverInputToken",
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
	title,
	extra,
}: MetricsSummaryCardProps) {
	const { t } = useTranslation();
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>{title ?? t("dashboard.metricOverview")}</CardTitle>
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
								label={t(item.labelKey)}
								value={item.format(data[item.key])}
								subLabel={t(item.subLabelKey)}
							/>
						))}
					</div>
				) : (
					<div className="flex h-[160px] items-center justify-center text-xs text-muted-foreground">
						{t("overview.dataLoadFailed")}
					</div>
				)}
				{extra}
			</CardContent>
		</Card>
	);
}
