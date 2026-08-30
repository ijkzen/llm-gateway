import { SegmentedControl } from "@/components/segmented-control";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useInView } from "@/hooks/use-in-view";
import {
	type RaceSort,
	type RaceSortKey,
	type VirtualModelRankItem,
	useVirtualModelRace,
} from "@/hooks/use-virtual-model-race";
import {
	type RacePeriod,
	formatPeriodLabel,
	periodBounds,
	toLocalInputValue,
} from "@/lib/race-period";
import { formatTokenCount } from "@/lib/utils";
import { ArrowDown, ArrowUp, ChevronLeft, ChevronRight, Layers } from "lucide-react";
import { useState } from "react";

const PERIOD_OPTIONS = [
	{ value: "day", label: "天" },
	{ value: "week", label: "周" },
	{ value: "month", label: "月" },
	{ value: "year", label: "年" },
	{ value: "custom", label: "自定义" },
] as const satisfies readonly { value: RacePeriod | "custom"; label: string }[];

/** 6 列指标定义：key / 标题 / 格式化 / 默认方向（true=降序，耗时类默认升序）。 */
const COLUMNS: ReadonlyArray<{
	key: RaceSortKey;
	label: string;
	format: (v: number) => string;
	defaultDesc: boolean;
}> = [
	{ key: "totalTokens", label: "总计 Token", format: formatTokenCount, defaultDesc: true },
	{ key: "requestCount", label: "请求数", format: (v) => v.toLocaleString(), defaultDesc: true },
	{ key: "ttft", label: "TTFT", format: (v) => `${v.toFixed(1)} ms`, defaultDesc: false },
	{
		key: "requestTime",
		label: "平均耗时",
		format: (v) => `${v.toFixed(1)} ms`,
		defaultDesc: false,
	},
	{ key: "tps", label: "TPS", format: (v) => v.toFixed(2), defaultDesc: true },
	{
		key: "cacheHitRate",
		label: "缓存命中率",
		format: (v) => `${(v * 100).toFixed(1)}%`,
		defaultDesc: true,
	},
];

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

/**
 * 虚拟模型赛马卡片：规格与供应商赛马完全一致——单卡片聚合展示 6 个指标
 * （总计 Token / 请求数 / TTFT / 平均耗时 / TPS / 缓存命中率），可点表头
 * 按任意指标升/降序；时间窗口天/周/月/年（左右箭头切换周期）+ 自定义（秒级）。
 * 卡片进入视口才发请求。
 */
export function VirtualModelRaceCard() {
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

	// 排序：默认按总计 Token 降序；点击表头切换升/降。
	const [sort, setSort] = useState<RaceSort>({ sortBy: "totalTokens", sortOrder: "desc" });

	const window =
		period === "custom"
			? (appliedCustom ?? { startTime: customStart, endTime: customEnd })
			: periodBounds(period, offset, now);

	const { ref, inView } = useInView();
	const query = useVirtualModelRace(window, sort, inView);

	const changePeriod = (next: RacePeriod | "custom") => {
		setPeriod(next);
		if (next === "custom") {
			// 进入自定义：用当前输入值作为初始窗口并立即生效。
			setAppliedCustom({ startTime: customStart, endTime: customEnd });
		}
	};

	const handleSort = (key: RaceSortKey) => {
		setSort((prev) => {
			if (prev.sortBy === key) {
				return { ...prev, sortOrder: prev.sortOrder === "asc" ? "desc" : "asc" };
			}
			const column = COLUMNS.find((c) => c.key === key);
			return { sortBy: key, sortOrder: column?.defaultDesc ? "desc" : "asc" };
		});
	};

	return (
		<div
			ref={ref}
			className="rounded-2xl border border-white/70 bg-white/65 p-5 shadow-[0_4px_16px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04]"
		>
			<div className="mb-4 flex flex-wrap items-center gap-3">
				<span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
					<Layers className="h-4 w-4" />
				</span>
				<div className="min-w-0">
					<h3 className="text-sm font-semibold text-foreground">虚拟模型赛马</h3>
					<p className="truncate text-xs text-muted-foreground">
						{period === "custom" ? "自定义时间范围" : formatPeriodLabel(period, offset, now)}
					</p>
				</div>

				<div className="ml-auto flex flex-wrap items-center gap-2">
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
			</div>

			{period === "custom" && (
				<div className="mb-4 flex flex-wrap items-center gap-2">
					<Input
						type="datetime-local"
						step={1}
						data-testid="custom-start"
						className="h-8 w-auto text-xs"
						value={toLocalInputValue(customStart)}
						onChange={(e) => setCustomStart(parseLocalInput(e.target.value))}
					/>
					<span className="text-xs text-muted-foreground">~</span>
					<Input
						type="datetime-local"
						step={1}
						data-testid="custom-end"
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
				<div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">
					滚动到此处后加载
				</div>
			) : query.isLoading ? (
				<div className="h-[220px] animate-pulse rounded-lg bg-slate-200/60 dark:bg-white/5" />
			) : query.isError ? (
				<div className="flex h-[220px] flex-col items-center justify-center gap-2 text-xs text-muted-foreground">
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
				<div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">
					该时间段暂无数据
				</div>
			) : (
				<RaceTable items={query.data.items} sort={sort} onSort={handleSort} />
			)}
		</div>
	);
}

/** 可排序指标表格。 */
function RaceTable({
	items,
	sort,
	onSort,
}: {
	items: VirtualModelRankItem[];
	sort: RaceSort;
	onSort: (key: RaceSortKey) => void;
}) {
	return (
		<div className="overflow-x-auto">
			<table className="w-full min-w-[720px] border-collapse text-sm">
				<thead>
					<tr className="border-b border-foreground/10">
						<th className="w-10 px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							#
						</th>
						<th className="px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							虚拟模型
						</th>
						{COLUMNS.map((column) => {
							const active = sort.sortBy === column.key;
							return (
								<th key={column.key} className="px-2 py-2 text-right">
									<button
										type="button"
										onClick={() => onSort(column.key)}
										aria-label={column.label}
										className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium transition-colors hover:bg-foreground/5 ${
											active ? "text-foreground" : "text-muted-foreground"
										}`}
									>
										{column.label}
										{active &&
											(sort.sortOrder === "asc" ? (
												<ArrowUp data-testid={`sort-${column.key}`} className="h-3 w-3" />
											) : (
												<ArrowDown data-testid={`sort-${column.key}`} className="h-3 w-3" />
											))}
									</button>
								</th>
							);
						})}
					</tr>
				</thead>
				<tbody>
					{items.map((item, index) => (
						<tr
							key={item.virtualModelDisplayId}
							className="border-b border-foreground/5 last:border-0 hover:bg-foreground/5"
						>
							<td className="px-2 py-2 text-left font-mono text-xs text-muted-foreground">
								{index + 1}
							</td>
							<td className="px-2 py-2 text-left font-medium text-foreground">
								{item.virtualModelDisplayId || "未知模型"}
							</td>
							{COLUMNS.map((column) => (
								<td
									key={column.key}
									className="px-2 py-2 text-right font-mono text-xs tabular-nums text-foreground"
								>
									{column.format(item[column.key])}
								</td>
							))}
						</tr>
					))}
				</tbody>
			</table>
		</div>
	);
}
