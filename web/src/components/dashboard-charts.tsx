import {
	ChartContainer,
	ChartLegend,
	ChartTooltip,
	ChartTooltipContent,
} from "@/components/ui/chart";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { ModelValue, TrendPoint } from "@/hooks/use-dashboard-stats";
import i18n from "@/i18n";
import type { ChartGranularity } from "@/lib/race-period";
import { cn, middleEllipsis, topWithOther } from "@/lib/utils";
import { useState } from "react";
import {
	Bar,
	BarChart,
	CartesianGrid,
	Cell,
	Line,
	LineChart,
	Pie,
	PieChart,
	XAxis,
	YAxis,
} from "recharts";

export const CHART_COLORS = [
	"hsl(var(--chart-1))",
	"hsl(var(--chart-2))",
	"hsl(var(--chart-3))",
	"hsl(var(--chart-4))",
	"hsl(var(--chart-5))",
] as const;

export const OTHER_LABEL = "其他";

/** 当前语言的「其他」标签（Top N + 其他聚合项）。 */
export function otherLabel(): string {
	return i18n.t("dashboard.other");
}

export function chartColorAt(index: number): string {
	return CHART_COLORS[index % CHART_COLORS.length] ?? "hsl(var(--chart-1))";
}

/** 展示标签：供应商・模型（供应商缺失时退化为模型名）。 */
export function modelLabel(item: Pick<ModelValue, "providerName" | "modelId">): string {
	return item.providerName ? `${item.providerName}・${item.modelId}` : item.modelId;
}

interface ChartItem extends ModelValue {
	/** 唯一展示标签（provider.name 唯一 ⇒ 供应商・模型 全局唯一）。 */
	label: string;
}

function toChartItems(items: ModelValue[]): ChartItem[] {
	return items.map((item) => ({ ...item, label: modelLabel(item) }));
}

/** Top 10 + 其他（降序）。 */
export function toRankedModels(items: ModelValue[]): ChartItem[] {
	const other = otherLabel();
	return topWithOther(toChartItems(items), {
		providerName: "",
		modelId: OTHER_LABEL,
		label: other,
		value: 0,
	});
}

/** 按桶粒度格式化 X 轴标签：小时 → HH:00，天 → M月d日，月 → yyyy年M月，年 → yyyy年。 */
export function formatBucketLabel(bucketStart: number, granularity: ChartGranularity): string {
	const date = new Date(bucketStart);
	const zh = i18n.language.startsWith("zh");
	switch (granularity) {
		case "hour":
			return `${date.getHours().toString().padStart(2, "0")}:00`;
		case "day":
			return zh
				? `${date.getMonth() + 1}月${date.getDate()}日`
				: date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
		case "month":
			return zh
				? `${date.getFullYear()}年${date.getMonth() + 1}月`
				: date.toLocaleDateString("en-US", { month: "short", year: "numeric" });
		case "year":
			return zh ? `${date.getFullYear()}年` : `${date.getFullYear()}`;
	}
}

/** 由相邻桶间距推断粒度：1h=小时桶，24h=天桶，否则月桶。 */
export function inferGranularity(bucketStart: number[]): "hour" | "day" | "month" {
	const first = bucketStart[0];
	const second = bucketStart[1];
	if (first !== undefined && second !== undefined) {
		const gap = second - first;
		if (gap <= 3_600_000) return "hour";
		if (gap <= 24 * 3_600_000) return "day";
	}
	return "month";
}

interface TrendLineChartProps {
	data: TrendPoint[];
	label: string;
	formatValue?: (value: number) => string;
	/** 显式桶粒度（后端分桶结果已知时传入，避免由间距推断）。 */
	granularity?: ChartGranularity;
}

/** 按时间窗口分桶的单条总量折线（粒度可显式指定，缺省由数据推断）。 */
export function TrendLineChart({ data, label, formatValue, granularity }: TrendLineChartProps) {
	const resolvedGranularity = granularity ?? inferGranularity(data.map((p) => p.bucketStart));
	const chartData = data.map((point) => ({
		label: formatBucketLabel(point.bucketStart, resolvedGranularity),
		value: point.value,
	}));
	// 标签密度自适应：约每 6 个点显示一个标签，避免 24 点小时图过密 / 7 点周图过疏。
	const labelInterval = Math.max(0, Math.floor(chartData.length / 6) - 1);
	return (
		<ChartContainer
			config={{ value: { label, color: "hsl(var(--chart-1))" } }}
			className="h-[260px] w-full"
		>
			<LineChart data={chartData} margin={{ left: 8, right: 8, top: 8 }}>
				<CartesianGrid vertical={false} />
				<XAxis
					dataKey="label"
					tickLine={false}
					axisLine={false}
					interval={labelInterval}
					tickMargin={8}
				/>
				<YAxis
					tickLine={false}
					axisLine={false}
					allowDecimals={false}
					width={44}
					tickFormatter={formatValue}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{formatValue ? formatValue(Number(value)) : Number(value).toLocaleString()}
								</span>
							)}
						/>
					}
				/>
				<Line
					type="monotone"
					dataKey="value"
					stroke="var(--color-value)"
					strokeWidth={2}
					dot={false}
				/>
			</LineChart>
		</ChartContainer>
	);
}

interface ModelChartProps {
	data: ModelValue[];
	formatValue?: (value: number) => string;
}

function formatValueText(value: number, formatValue?: (value: number) => string): string {
	return formatValue ? formatValue(value) : value.toLocaleString();
}

function toPieConfig(data: ChartItem[]) {
	return Object.fromEntries(data.map((item) => [item.label, { label: item.label }]));
}

/** 图例项：色块 + 中间省略的「供应商・模型」文本，hover 显示完整名称。 */
function ModelLegendItem({
	label,
	color,
	active,
}: {
	label: string;
	color?: string;
	active?: boolean;
}) {
	return (
		<div className="flex items-center gap-1.5 [&>svg]:h-3 [&>svg]:w-3 [&>svg]:text-muted-foreground">
			<div className="h-2 w-2 shrink-0 rounded-[2px]" style={{ backgroundColor: color }} />
			<Tooltip>
				<TooltipTrigger asChild>
					<span
						className={cn(
							"cursor-default transition-colors",
							active && "font-medium underline decoration-2 underline-offset-4",
						)}
					>
						{middleEllipsis(label, 18)}
					</span>
				</TooltipTrigger>
				<TooltipContent side="top">{label}</TooltipContent>
			</Tooltip>
		</div>
	);
}

/** 饼图 / 条形图共享的图例内容：取「供应商・模型」组合 label 作唯一 key。 */
function ModelLegendContent({
	payload,
	activeLabel,
}: {
	payload?: Array<{ value: string; color?: string }>;
	activeLabel?: string;
}) {
	if (!payload?.length) {
		return null;
	}
	return (
		<div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1.5 pt-3">
			{payload.map((item) => (
				<ModelLegendItem
					key={item.value}
					label={item.value}
					color={item.color}
					active={activeLabel === item.value}
				/>
			))}
		</div>
	);
}

/** 占比计算（占比 ×100，保留 3 位小数，去尾零），供选中扇区展示。 */
function percentText(value: number, total: number): string {
	if (total <= 0) {
		return "";
	}
	const percent = Number.parseFloat(((value / total) * 100).toFixed(3));
	return `${percent}%`;
}

/** 按模型占比的饼图（Top 10 + 其他）。 */
export function ModelPieChart({ data, formatValue }: ModelChartProps) {
	const ranked = toRankedModels(data);
	const total = ranked.reduce((sum, item) => sum + item.value, 0);
	const [activeLabel, setActiveLabel] = useState<string | null>(null);

	return (
		<ChartContainer config={toPieConfig(ranked)} className="mx-auto h-[260px] w-full">
			<PieChart>
				<ChartTooltip
					content={
						<ChartTooltipContent
							labelKey="label"
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{formatValueText(Number(value), formatValue)}
									<span className="ml-1 text-muted-foreground">
										{percentText(Number(value), total)}
									</span>
								</span>
							)}
						/>
					}
				/>
				<Pie
					data={ranked}
					dataKey="value"
					nameKey="label"
					strokeWidth={2}
					outerRadius={110}
					onClick={(entry) => setActiveLabel((prev) => (prev === entry.label ? null : entry.label))}
					style={{ cursor: "pointer", outline: "none" }}
				>
					{ranked.map((item, index) => (
						<Cell key={item.label} fill={chartColorAt(index)} />
					))}
				</Pie>
				<ChartLegend content={<ModelLegendContent activeLabel={activeLabel ?? undefined} />} />
			</PieChart>
		</ChartContainer>
	);
}

/** 按模型降序的横向条形图（Top 10 + 其他）。 */
export function ModelRankBarChart({ data, formatValue }: ModelChartProps) {
	const ranked = toRankedModels(data);
	const height = Math.max(200, ranked.length * 36 + 16);
	return (
		<ChartContainer config={toPieConfig(ranked)} className="w-full" style={{ height }}>
			<BarChart data={ranked} layout="vertical" margin={{ left: 8, right: 24, top: 4 }}>
				<XAxis type="number" hide />
				<YAxis
					type="category"
					dataKey="label"
					tickLine={false}
					axisLine={false}
					width={120}
					tickFormatter={(label: string) => middleEllipsis(label, 16)}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							labelKey="label"
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{formatValueText(Number(value), formatValue)}
								</span>
							)}
						/>
					}
				/>
				<Bar dataKey="value" radius={[0, 4, 4, 0]}>
					{ranked.map((item, index) => (
						<Cell key={item.label} fill={chartColorAt(index)} />
					))}
				</Bar>
			</BarChart>
		</ChartContainer>
	);
}
