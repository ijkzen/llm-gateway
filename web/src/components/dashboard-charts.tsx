import {
	ChartContainer,
	ChartLegend,
	ChartLegendContent,
	ChartTooltip,
	ChartTooltipContent,
} from "@/components/ui/chart";
import type { ModelValue, TrendPoint } from "@/hooks/use-dashboard-stats";
import { topWithOther } from "@/lib/utils";
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

/** Top 10 + 其他（降序）。 */
export function toRankedModels(items: ModelValue[]): ModelValue[] {
	return topWithOther(items, { modelId: OTHER_LABEL, value: 0 });
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

function toPieConfig(data: ModelValue[]) {
	return Object.fromEntries(data.map((item) => [item.modelId, { label: item.modelId }]));
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
							nameKey="modelId"
							hideLabel
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{formatValue ? formatValue(Number(value)) : Number(value).toLocaleString()}
								</span>
							)}
						/>
					}
				/>
				<Pie data={ranked} dataKey="value" nameKey="modelId" strokeWidth={2}>
					{ranked.map((item, index) => (
						<Cell key={item.modelId} fill={chartColorAt(index)} />
					))}
				</Pie>
				<ChartLegend content={<ChartLegendContent nameKey="modelId" />} />
			</PieChart>
		</ChartContainer>
	);
}

function truncateModelId(modelId: string): string {
	return modelId.length > 16 ? `${modelId.slice(0, 15)}…` : modelId;
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
					dataKey="modelId"
					tickLine={false}
					axisLine={false}
					width={120}
					tickFormatter={truncateModelId}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							hideLabel
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{formatValue ? formatValue(Number(value)) : Number(value).toLocaleString()}
								</span>
							)}
						/>
					}
				/>
				<Bar dataKey="value" radius={[0, 4, 4, 0]}>
					{ranked.map((item, index) => (
						<Cell key={item.modelId} fill={chartColorAt(index)} />
					))}
				</Bar>
			</BarChart>
		</ChartContainer>
	);
}
