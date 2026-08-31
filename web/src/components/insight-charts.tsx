import {
	ChartContainer,
	ChartLegend,
	ChartTooltip,
	ChartTooltipContent,
} from "@/components/ui/chart";
import type { FloatTrendPoint, PercentilePoint, TrendPoint } from "@/hooks/use-dashboard-insight";
import i18n from "@/i18n";
import type { ChartGranularity } from "@/lib/race-period";
import { formatPercent } from "@/lib/utils";
import {
	Area,
	AreaChart,
	Bar,
	BarChart,
	CartesianGrid,
	Cell,
	Line,
	LineChart,
	XAxis,
	YAxis,
} from "recharts";

/** 图表配色（与 dashboard-charts 的 CHART_COLORS 同源，避免跨模块耦合）。 */
const CHART_COLORS = [
	"hsl(var(--chart-1))",
	"hsl(var(--chart-2))",
	"hsl(var(--chart-3))",
	"hsl(var(--chart-4))",
	"hsl(var(--chart-5))",
] as const;

function chartColorAt(index: number): string {
	return CHART_COLORS[index % CHART_COLORS.length] ?? "hsl(var(--chart-1))";
}

/** 按桶粒度格式化 X 轴标签（与 dashboard-charts 的 formatBucketLabel 同语义）。 */
function formatBucketLabel(bucketStart: number, granularity: ChartGranularity): string {
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

/** 时间桶标签（复用 dashboard-charts 的格式化与粒度推断）。 */
function bucketLabelData(
	data: Array<{ bucketStart: number }>,
	granularity?: ChartGranularity,
): string[] {
	const gap = data.length > 1 ? (data[1]?.bucketStart ?? 0) - (data[0]?.bucketStart ?? 0) : 0;
	const resolved: ChartGranularity =
		granularity ?? (gap <= 3_600_000 ? "hour" : gap <= 24 * 3_600_000 ? "day" : "month");
	return data.map((p) => formatBucketLabel(p.bucketStart, resolved));
}

function labelInterval(count: number): number {
	return Math.max(0, Math.floor(count / 6) - 1);
}

/** 延迟分位多线图：同一时间轴的 P50/P90/P95/P99。 */
export function PercentileLineChart({
	data,
	granularity,
}: {
	data: PercentilePoint[];
	granularity?: ChartGranularity;
}) {
	const labels = bucketLabelData(data, granularity);
	const chartData = data.map((point, index) => ({
		label: labels[index],
		p50: point.p50,
		p90: point.p90,
		p95: point.p95,
		p99: point.p99,
	}));
	return (
		<ChartContainer
			config={{
				p50: { label: "P50", color: "hsl(var(--chart-1))" },
				p90: { label: "P90", color: "hsl(var(--chart-2))" },
				p95: { label: "P95", color: "hsl(var(--chart-3))" },
				p99: { label: "P99", color: "hsl(var(--chart-4))" },
			}}
			className="h-[260px] w-full"
		>
			<LineChart data={chartData} margin={{ left: 8, right: 8, top: 8 }}>
				<CartesianGrid vertical={false} />
				<XAxis
					dataKey="label"
					tickLine={false}
					axisLine={false}
					interval={labelInterval(chartData.length)}
					tickMargin={8}
				/>
				<YAxis
					tickLine={false}
					axisLine={false}
					width={52}
					tickFormatter={(v: number) => `${v} ms`}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{Number(value).toFixed(0)} ms
								</span>
							)}
						/>
					}
				/>
				<Line type="monotone" dataKey="p50" stroke="var(--color-p50)" strokeWidth={2} dot={false} />
				<Line type="monotone" dataKey="p90" stroke="var(--color-p90)" strokeWidth={2} dot={false} />
				<Line type="monotone" dataKey="p95" stroke="var(--color-p95)" strokeWidth={2} dot={false} />
				<Line type="monotone" dataKey="p99" stroke="var(--color-p99)" strokeWidth={2} dot={false} />
				<ChartLegend />
			</LineChart>
		</ChartContainer>
	);
}

/** 失败趋势：成功/失败堆叠面积（主轴）+ 失败率折线（次轴）。 */
export function FailureTrendChart({
	callTrend,
	failureTrend,
	failureRateTrend,
	granularity,
}: {
	callTrend: TrendPoint[];
	failureTrend: TrendPoint[];
	failureRateTrend: FloatTrendPoint[];
	granularity?: ChartGranularity;
}) {
	const labels = bucketLabelData(callTrend, granularity);
	// 以调用趋势为主轴基准（三组长度一致），堆叠面积 = 成功数 + 失败数。
	const chartData = callTrend.map((point, index) => ({
		label: labels[index],
		success: Math.max(0, point.value - (failureTrend[index]?.value ?? 0)),
		failed: failureTrend[index]?.value ?? 0,
		failureRate: failureRateTrend[index]?.value ?? 0,
	}));
	return (
		<ChartContainer
			config={{
				success: { label: "成功", color: "hsl(var(--chart-2))" },
				failed: { label: "失败", color: "hsl(var(--chart-5))" },
				failureRate: { label: "失败率", color: "hsl(var(--chart-1))" },
			}}
			className="h-[260px] w-full"
		>
			<AreaChart data={chartData} margin={{ left: 8, right: 8, top: 8 }}>
				<CartesianGrid vertical={false} />
				<XAxis
					dataKey="label"
					tickLine={false}
					axisLine={false}
					interval={labelInterval(chartData.length)}
					tickMargin={8}
				/>
				<YAxis yAxisId="count" tickLine={false} axisLine={false} width={44} allowDecimals={false} />
				<YAxis
					yAxisId="rate"
					orientation="right"
					tickLine={false}
					axisLine={false}
					width={40}
					domain={[0, 1]}
					tickFormatter={(v: number) => `${Math.round(v * 100)}%`}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							formatter={(value, name) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{name === "failureRate"
										? formatPercent(Number(value))
										: Number(value).toLocaleString()}
								</span>
							)}
						/>
					}
				/>
				<Area
					yAxisId="count"
					type="monotone"
					dataKey="success"
					stackId="1"
					stroke="var(--color-success)"
					fill="var(--color-success)"
					fillOpacity={0.5}
				/>
				<Area
					yAxisId="count"
					type="monotone"
					dataKey="failed"
					stackId="1"
					stroke="var(--color-failed)"
					fill="var(--color-failed)"
					fillOpacity={0.5}
				/>
				<Line
					yAxisId="rate"
					type="monotone"
					dataKey="failureRate"
					stroke="var(--color-failureRate)"
					strokeWidth={2}
					dot={false}
				/>
				<ChartLegend />
			</AreaChart>
		</ChartContainer>
	);
}

/** 失败原因分布横向条形图（空串原因显示「无原因」）。 */
export function FailureReasonBarChart({
	reasons,
	noReasonLabel,
}: {
	reasons: Array<{ reason: string; count: number }>;
	noReasonLabel: string;
}) {
	const data = reasons.map((item) => ({
		label: item.reason || noReasonLabel,
		value: item.count,
	}));
	const height = Math.max(200, data.length * 36 + 16);
	return (
		<ChartContainer
			config={Object.fromEntries(data.map((item) => [item.label, { label: item.label }]))}
			className="w-full"
			style={{ height }}
		>
			<BarChart data={data} layout="vertical" margin={{ left: 8, right: 24, top: 4 }}>
				<XAxis type="number" hide />
				<YAxis
					type="category"
					dataKey="label"
					tickLine={false}
					axisLine={false}
					width={130}
					tickFormatter={(label: string) => (label.length > 16 ? `${label.slice(0, 16)}…` : label)}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							labelKey="label"
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{Number(value).toLocaleString()}
								</span>
							)}
						/>
					}
				/>
				<Bar dataKey="value" radius={[0, 4, 4, 0]}>
					{data.map((item, index) => (
						<Cell key={item.label} fill={chartColorAt(index)} />
					))}
				</Bar>
			</BarChart>
		</ChartContainer>
	);
}

/** Token 结构：输入/输出堆叠面积 + 缓存命中率折线（次轴）。 */
export function TokenStructureChart({
	inputTokenTrend,
	outputTokenTrend,
	cacheHitRateTrend,
	granularity,
	formatValue,
}: {
	inputTokenTrend: TrendPoint[];
	outputTokenTrend: TrendPoint[];
	cacheHitRateTrend: FloatTrendPoint[];
	granularity?: ChartGranularity;
	formatValue?: (value: number) => string;
}) {
	const labels = bucketLabelData(inputTokenTrend, granularity);
	const chartData = inputTokenTrend.map((point, index) => ({
		label: labels[index],
		input: point.value,
		output: outputTokenTrend[index]?.value ?? 0,
		cacheHitRate: cacheHitRateTrend[index]?.value ?? 0,
	}));
	return (
		<ChartContainer
			config={{
				input: { label: "输入", color: "hsl(var(--chart-1))" },
				output: { label: "输出", color: "hsl(var(--chart-3))" },
				cacheHitRate: { label: "缓存命中率", color: "hsl(var(--chart-4))" },
			}}
			className="h-[260px] w-full"
		>
			<AreaChart data={chartData} margin={{ left: 8, right: 8, top: 8 }}>
				<CartesianGrid vertical={false} />
				<XAxis
					dataKey="label"
					tickLine={false}
					axisLine={false}
					interval={labelInterval(chartData.length)}
					tickMargin={8}
				/>
				<YAxis
					yAxisId="tokens"
					tickLine={false}
					axisLine={false}
					width={48}
					tickFormatter={formatValue}
				/>
				<YAxis
					yAxisId="rate"
					orientation="right"
					tickLine={false}
					axisLine={false}
					width={40}
					domain={[0, 1]}
					tickFormatter={(v: number) => `${Math.round(v * 100)}%`}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							formatter={(value, name) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{name === "cacheHitRate"
										? formatPercent(Number(value))
										: formatValue
											? formatValue(Number(value))
											: Number(value).toLocaleString()}
								</span>
							)}
						/>
					}
				/>
				<Area
					yAxisId="tokens"
					type="monotone"
					dataKey="input"
					stackId="1"
					stroke="var(--color-input)"
					fill="var(--color-input)"
					fillOpacity={0.5}
				/>
				<Area
					yAxisId="tokens"
					type="monotone"
					dataKey="output"
					stackId="1"
					stroke="var(--color-output)"
					fill="var(--color-output)"
					fillOpacity={0.5}
				/>
				<Line
					yAxisId="rate"
					type="monotone"
					dataKey="cacheHitRate"
					stroke="var(--color-cacheHitRate)"
					strokeWidth={2}
					dot={false}
				/>
				<ChartLegend />
			</AreaChart>
		</ChartContainer>
	);
}

/** 吞吐：RPM/TPM 双折线 + 流式占比（次轴）。 */
export function ThroughputChart({
	rpmTrend,
	tpmTrend,
	streamRatioTrend,
	granularity,
	formatValue,
}: {
	rpmTrend: TrendPoint[];
	tpmTrend: TrendPoint[];
	streamRatioTrend: FloatTrendPoint[];
	granularity?: ChartGranularity;
	formatValue?: (value: number) => string;
}) {
	const labels = bucketLabelData(rpmTrend, granularity);
	const chartData = rpmTrend.map((point, index) => ({
		label: labels[index],
		rpm: point.value,
		tpm: tpmTrend[index]?.value ?? 0,
		streamRatio: streamRatioTrend[index]?.value ?? 0,
	}));
	return (
		<ChartContainer
			config={{
				rpm: { label: "RPM", color: "hsl(var(--chart-1))" },
				tpm: { label: "TPM", color: "hsl(var(--chart-2))" },
				streamRatio: { label: "流式占比", color: "hsl(var(--chart-3))" },
			}}
			className="h-[260px] w-full"
		>
			<LineChart data={chartData} margin={{ left: 8, right: 8, top: 8 }}>
				<CartesianGrid vertical={false} />
				<XAxis
					dataKey="label"
					tickLine={false}
					axisLine={false}
					interval={labelInterval(chartData.length)}
					tickMargin={8}
				/>
				<YAxis
					yAxisId="count"
					tickLine={false}
					axisLine={false}
					width={48}
					tickFormatter={formatValue}
				/>
				<YAxis
					yAxisId="rate"
					orientation="right"
					tickLine={false}
					axisLine={false}
					width={40}
					domain={[0, 1]}
					tickFormatter={(v: number) => `${Math.round(v * 100)}%`}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							formatter={(value, name) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{name === "streamRatio"
										? formatPercent(Number(value))
										: formatValue
											? formatValue(Number(value))
											: Number(value).toLocaleString()}
								</span>
							)}
						/>
					}
				/>
				<Line
					yAxisId="count"
					type="monotone"
					dataKey="rpm"
					stroke="var(--color-rpm)"
					strokeWidth={2}
					dot={false}
				/>
				<Line
					yAxisId="count"
					type="monotone"
					dataKey="tpm"
					stroke="var(--color-tpm)"
					strokeWidth={2}
					dot={false}
				/>
				<Line
					yAxisId="rate"
					type="monotone"
					dataKey="streamRatio"
					stroke="var(--color-streamRatio)"
					strokeWidth={2}
					dot={false}
				/>
				<ChartLegend />
			</LineChart>
		</ChartContainer>
	);
}

/** 输出 token/秒 折线（Token 结构 Tab 内第二张图）。 */
export function OutputPerSecLineChart({
	data,
	granularity,
}: {
	data: FloatTrendPoint[];
	granularity?: ChartGranularity;
}) {
	const labels = bucketLabelData(data, granularity);
	const chartData = data.map((point, index) => ({
		label: labels[index],
		value: point.value,
	}));
	return (
		<ChartContainer
			config={{ value: { label: "token/s", color: "hsl(var(--chart-2))" } }}
			className="h-[220px] w-full"
		>
			<LineChart data={chartData} margin={{ left: 8, right: 8, top: 8 }}>
				<CartesianGrid vertical={false} />
				<XAxis
					dataKey="label"
					tickLine={false}
					axisLine={false}
					interval={labelInterval(chartData.length)}
					tickMargin={8}
				/>
				<YAxis tickLine={false} axisLine={false} width={48} />
				<ChartTooltip
					content={
						<ChartTooltipContent
							formatter={(value) => (
								<span className="font-mono font-medium tabular-nums text-foreground">
									{Number(value).toFixed(1)} token/s
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
