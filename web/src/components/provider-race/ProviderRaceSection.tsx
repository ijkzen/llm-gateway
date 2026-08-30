import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { formatTokenCount } from "@/lib/utils";
import { Coins, Gauge, Timer } from "lucide-react";

/**
 * 供应商赛马区块：三张卡片（Token 总额 / TPS / TTFT），各自独立控制
 * 时间窗口（天/周/月/年/自定义），卡片滚动到视口才发起请求。
 */
export function ProviderRaceSection() {
	return (
		<Card>
			<CardHeader>
				<CardTitle>供应商赛马</CardTitle>
			</CardHeader>
			<CardContent className="grid grid-cols-1 gap-4 lg:grid-cols-3">
				<ProviderRaceCard
					metric="token"
					title="Token 总额"
					description="各供应商成功请求的 token 总量"
					icon={<Coins className="h-4 w-4" />}
					formatValue={formatTokenCount}
				/>
				<ProviderRaceCard
					metric="tps"
					title="TPS"
					description="输出 token / 网络耗时（加权均值）"
					icon={<Gauge className="h-4 w-4" />}
					formatValue={(v) => v.toFixed(2)}
				/>
				<ProviderRaceCard
					metric="ttft"
					title="TTFT"
					description="流式首 token 耗时（越小越快）"
					icon={<Timer className="h-4 w-4" />}
					formatValue={(v) => `${v.toFixed(1)} ms`}
				/>
			</CardContent>
		</Card>
	);
}
