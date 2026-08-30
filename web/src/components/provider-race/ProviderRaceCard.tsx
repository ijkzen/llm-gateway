import { ProviderRankBarChart } from "@/components/provider-race/ProviderRankBarChart";
import { SegmentedControl } from "@/components/segmented-control";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useInView } from "@/hooks/use-in-view";
import { type RaceMetric, useProviderRace } from "@/hooks/use-provider-race";
import {
	type RacePeriod,
	formatPeriodLabel,
	periodBounds,
	toLocalInputValue,
} from "@/lib/race-period";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useState } from "react";

const PERIOD_OPTIONS = [
	{ value: "day", label: "天" },
	{ value: "week", label: "周" },
	{ value: "month", label: "月" },
	{ value: "year", label: "年" },
	{ value: "custom", label: "自定义" },
] as const satisfies readonly { value: RacePeriod | "custom"; label: string }[];

interface ProviderRaceCardProps {
	metric: RaceMetric;
	title: string;
	description: string;
	icon: React.ReactNode;
	formatValue: (value: number) => string;
}

/**
 * 单张赛马卡片：窗口控制（天/周/月/年/自定义 + 方向键）+ 懒加载排行。
 * 卡片进入视口才发起请求（IntersectionObserver），窗口切换即重新查询。
 */
export function ProviderRaceCard({
	metric,
	title,
	description,
	icon,
	formatValue,
}: ProviderRaceCardProps) {
	// 挂载时刻固化 now：保证「当前周期」的窗口终点稳定，不因渲染抖动重复请求。
	const [now] = useState(() => Date.now());
	const [period, setPeriod] = useState<RacePeriod | "custom">("day");
	const [offset, setOffset] = useState(0);
	// 自定义窗口：默认最近 1 小时，秒级精度由 datetime-local step=1 保证。
	const [customStart, setCustomStart] = useState(() => Date.now() - 3_600_000);
	const [customEnd, setCustomEnd] = useState(() => Date.now());
	const [appliedCustom, setAppliedCustom] = useState<{ startTime: number; endTime: number } | null>(
		null,
	);

	const window =
		period === "custom"
			? (appliedCustom ?? { startTime: customStart, endTime: customEnd })
			: periodBounds(period, offset, now);

	const { ref, inView } = useInView();
	const query = useProviderRace(metric, window, inView);

	const changePeriod = (next: RacePeriod | "custom") => {
		setPeriod(next);
		if (next === "custom") {
			// 进入自定义：用当前输入值作为初始窗口并立即生效。
			setAppliedCustom({ startTime: customStart, endTime: customEnd });
		}
	};

	return (
		<div
			ref={ref}
			className="rounded-2xl border border-white/70 bg-white/65 p-4 shadow-[0_4px_16px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04]"
		>
			<div className="mb-3 flex items-center gap-2">
				<span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
					{icon}
				</span>
				<div className="min-w-0">
					<h3 className="text-sm font-semibold text-foreground">{title}</h3>
					<p className="truncate text-xs text-muted-foreground">{description}</p>
				</div>
			</div>

			<div className="mb-3 flex flex-wrap items-center justify-between gap-2">
				<SegmentedControl options={PERIOD_OPTIONS} value={period} onChange={changePeriod} />
				{period === "custom" ? (
					<span className="text-xs text-muted-foreground">
						{formatDateTime(customStart)} ~ {formatDateTime(customEnd)}
					</span>
				) : (
					<div className="flex items-center gap-1">
						<Button
							variant="ghost"
							size="icon"
							className="h-6 w-6"
							aria-label="上一周期"
							onClick={() => setOffset((o) => o - 1)}
						>
							<ChevronLeft className="h-4 w-4" />
						</Button>
						<span className="min-w-24 text-center text-xs font-medium text-foreground">
							{formatPeriodLabel(period, offset, now)}
						</span>
						<Button
							variant="ghost"
							size="icon"
							className="h-6 w-6"
							aria-label="下一周期"
							onClick={() => setOffset((o) => o + 1)}
						>
							<ChevronRight className="h-4 w-4" />
						</Button>
					</div>
				)}
			</div>

			{period === "custom" && (
				<div className="mb-3 flex flex-wrap items-center gap-2">
					<Input
						type="datetime-local"
						step={1}
						className="h-8 w-auto text-xs"
						value={toLocalInputValue(customStart)}
						onChange={(e) => setCustomStart(parseLocalInput(e.target.value))}
					/>
					<span className="text-xs text-muted-foreground">~</span>
					<Input
						type="datetime-local"
						step={1}
						className="h-8 w-auto text-xs"
						value={toLocalInputValue(customEnd)}
						onChange={(e) => setCustomEnd(parseLocalInput(e.target.value))}
					/>
					<Button
						variant="outline"
						size="sm"
						className="h-8 text-xs"
						onClick={() => {
							if (customEnd > customStart) {
								setAppliedCustom({ startTime: customStart, endTime: customEnd });
							}
						}}
					>
						应用
					</Button>
				</div>
			)}

			{!inView ? (
				<div className="flex h-[200px] items-center justify-center text-xs text-muted-foreground">
					滚动到此处后加载
				</div>
			) : query.isLoading ? (
				<div className="h-[200px] animate-pulse rounded-lg bg-slate-200/60 dark:bg-white/5" />
			) : query.isError ? (
				<div className="flex h-[200px] flex-col items-center justify-center gap-2 text-xs text-muted-foreground">
					<span>数据加载失败</span>
					<button
						type="button"
						className="rounded-full bg-foreground/5 px-3 py-1 text-xs font-medium hover:bg-foreground/10"
						onClick={() => query.refetch()}
					>
						重试
					</button>
				</div>
			) : !query.data || query.data.items.length === 0 ? (
				<div className="flex h-[200px] items-center justify-center text-xs text-muted-foreground">
					该时间段暂无数据
				</div>
			) : (
				<ProviderRankBarChart items={query.data.items} formatValue={formatValue} />
			)}
		</div>
	);
}

function formatDateTime(ms: number): string {
	const date = new Date(ms);
	const pad = (n: number) => n.toString().padStart(2, "0");
	return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** datetime-local 值 → 毫秒时间戳；空值返回当前时刻（避免 NaN）。 */
function parseLocalInput(value: string): number {
	const ms = new Date(value).getTime();
	return Number.isNaN(ms) ? Date.now() : ms;
}
