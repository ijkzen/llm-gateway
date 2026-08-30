import {
	ChartContainer,
	ChartLegend,
	ChartTooltip,
	ChartTooltipContent,
} from "@/components/ui/chart";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { ModelValue, TrendPoint } from "@/hooks/use-dashboard-stats";
import { middleEllipsis, topWithOther } from "@/lib/utils";
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
	return topWithOther(toChartItems(items), {
		providerName: "",
		modelId: OTHER_LABEL,
		label: OTHER_LABEL,
		value: 0,
	});
}

function formatHourLabel(bucketStart: number): string {
	const date = new Date(bucketStart);
	return `${date.getHours().toString().padStart(2, "0")}:00`;
}

interface TrendLineChartProps {
	data: TrendPoint[];
	label: string;
	formatValue?: (value: number) => string;
}

/** 过去 24 小时按小时分桶的单条总量折线。 */
export function TrendLineChart({ data, label, formatValue }: TrendLineChartProps) {
	const chartData = data.map((point) => ({
		label: formatHourLabel(point.bucketStart),
		value: point.value,
	}));
	return (
		<ChartContainer
			config={{ value: { label, color: "hsl(var(--chart-1))" } }}
			className="h-[260px] w-full"
		>
			<LineChart data={chartData} margin={{ left: 8, right: 8, top: 8 }}>
				<CartesianGrid vertical={false} />
				<XAxis dataKey="label" tickLine={false} axisLine={false} interval={3} tickMargin={8} />
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
function ModelLegendItem({ label, color }: { label: string; color?: string }) {
	return (
		<div className="flex items-center gap-1.5 [&>svg]:h-3 [&>svg]:w-3 [&>svg]:text-muted-foreground">
			<div className="h-2 w-2 shrink-0 rounded-[2px]" style={{ backgroundColor: color }} />
			<Tooltip>
				<TooltipTrigger asChild>
					<span className="cursor-default">{middleEllipsis(label, 18)}</span>
				</TooltipTrigger>
				<TooltipContent side="top">{label}</TooltipContent>
			</Tooltip>
		</div>
	);
}

/** 饼图 / 条形图共享的图例内容：取「供应商・模型」组合 label 作唯一 key。 */
function ModelLegendContent({
	payload,
}: {
	payload?: Array<{ value: string; color?: string }>;
}) {
	if (!payload?.length) {
		return null;
	}
	return (
		<div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1.5 pt-3">
			{payload.map((item) => (
				<ModelLegendItem key={item.value} label={item.value} color={item.color} />
			))}
		</div>
	);
}

/** 按模型占比的饼图（Top 10 + 其他）。 */
export function ModelPieChart({ data, formatValue }: ModelChartProps) {
	const ranked = toRankedModels(data);
	return (
		<ChartContainer config={toPieConfig(ranked)} className="mx-auto h-[260px] w-full">
			<PieChart>
				<ChartTooltip
					content={
						<ChartTooltipContent
							nameKey="label"
							formatter={(value, name) => (
								<div className="flex items-center gap-2">
									<span className="text-foreground">{name}</span>
									<span className="font-mono font-medium tabular-nums text-foreground">
										{formatValueText(Number(value), formatValue)}
									</span>
								</div>
							)}
						/>
					}
				/>
				<Pie data={ranked} dataKey="value" nameKey="label" strokeWidth={2}>
					{ranked.map((item, index) => (
						<Cell key={item.label} fill={chartColorAt(index)} />
					))}
				</Pie>
				<ChartLegend content={<ModelLegendContent />} />
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
							nameKey="label"
							formatter={(value, name) => (
								<div className="flex items-center gap-2">
									<span className="text-foreground">{name}</span>
									<span className="font-mono font-medium tabular-nums text-foreground">
										{formatValueText(Number(value), formatValue)}
									</span>
								</div>
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
