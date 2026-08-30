import { chartColorAt } from "@/components/dashboard-charts";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import type { ProviderRankItem } from "@/hooks/use-provider-race";
import { middleEllipsis } from "@/lib/utils";
import { Bar, BarChart, Cell, XAxis, YAxis } from "recharts";

interface ProviderRankBarChartProps {
	items: ProviderRankItem[];
	formatValue: (value: number) => string;
}

function toBarData(items: ProviderRankItem[]): Array<ProviderRankItem & { label: string }> {
	return items.map((item) => ({
		...item,
		// Y 轴标签带序号，便于看排名（供应商名中间省略）。
		label: `${item.providerName || "未知供应商"}`,
	}));
}

/** 供应商赛马横向条形图：后端已按值排序返回 Top 10，直接渲染。 */
export function ProviderRankBarChart({ items, formatValue }: ProviderRankBarChartProps) {
	const data = toBarData(items);
	const height = Math.max(200, data.length * 36 + 16);
	return (
		<ChartContainer
			config={Object.fromEntries(
				data.map((item) => [item.providerName, { label: item.providerName }]),
			)}
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
					width={110}
					tickFormatter={(label: string) => middleEllipsis(label, 14)}
				/>
				<ChartTooltip
					content={
						<ChartTooltipContent
							nameKey="label"
							formatter={(value, name) => (
								<div className="flex items-center gap-2">
									<span className="text-foreground">{name}</span>
									<span className="font-mono font-medium tabular-nums text-foreground">
										{formatValue(Number(value))}
									</span>
								</div>
							)}
						/>
					}
				/>
				<Bar dataKey="value" radius={[0, 4, 4, 0]}>
					{data.map((item, index) => (
						<Cell key={item.providerName} fill={chartColorAt(index)} />
					))}
				</Bar>
			</BarChart>
		</ChartContainer>
	);
}
