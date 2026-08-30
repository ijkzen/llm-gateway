import { SegmentedControl } from "@/components/segmented-control";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
	type RacePeriod,
	formatPeriodLabel,
	periodBounds,
	toLocalInputValue,
} from "@/lib/race-period";
import { ChevronLeft, ChevronRight } from "lucide-react";

const PERIOD_OPTIONS = [
	{ value: "day", label: "天" },
	{ value: "week", label: "周" },
	{ value: "month", label: "月" },
	{ value: "year", label: "年" },
	{ value: "custom", label: "自定义" },
] as const satisfies readonly { value: RacePeriod | "custom"; label: string }[];

export interface RaceWindowState {
	period: RacePeriod | "custom";
	offset: number;
	customStart: number;
	customEnd: number;
	/** 已应用的自定义窗口（null 时退化为输入值）。 */
	appliedCustom: { startTime: number; endTime: number } | null;
}

interface RaceWindowControlProps {
	state: RaceWindowState;
	/** 固化 now（当前周期标题用）。 */
	now: number;
	onChange: (patch: Partial<RaceWindowState>) => void;
	/** 是否显示当前窗口标题（默认 true）。 */
	showLabel?: boolean;
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

/**
 * 赛马时间窗口共享控件：天/周/月/年 SegmentedControl + 左右箭头切换周期 +
 * 自定义秒级起止输入。首页三张赛马卡片与二级页图表区块共用。
 */
export function RaceWindowControl({
	state,
	now,
	onChange,
	showLabel = true,
}: RaceWindowControlProps) {
	const { period, offset, customStart, customEnd } = state;

	const changePeriod = (next: RacePeriod | "custom") => {
		onChange({ period: next });
		if (next === "custom") {
			// 进入自定义：用当前输入值作为初始窗口并立即生效。
			onChange({ appliedCustom: { startTime: customStart, endTime: customEnd } });
		}
	};

	return (
		<div className="flex flex-wrap items-center gap-2">
			<SegmentedControl options={PERIOD_OPTIONS} value={period} onChange={changePeriod} />
			{period === "custom" ? (
				showLabel && (
					<span className="text-xs text-muted-foreground">
						{formatDateTime(customStart)} ~ {formatDateTime(customEnd)}
					</span>
				)
			) : (
				<div className="flex items-center gap-1">
					<Button
						variant="ghost"
						size="icon"
						className="h-6 w-6"
						aria-label="上一周期"
						onClick={() => onChange({ offset: offset - 1 })}
					>
						<ChevronLeft className="h-4 w-4" />
					</Button>
					{showLabel && (
						<span className="min-w-24 text-center text-xs font-medium text-foreground">
							{formatPeriodLabel(period, offset, now)}
						</span>
					)}
					<Button
						variant="ghost"
						size="icon"
						className="h-6 w-6"
						aria-label="下一周期"
						onClick={() => onChange({ offset: offset + 1 })}
					>
						<ChevronRight className="h-4 w-4" />
					</Button>
				</div>
			)}

			{period === "custom" && (
				<div className="flex flex-wrap items-center gap-2">
					<Input
						type="datetime-local"
						step={1}
						data-testid="custom-start"
						className="h-8 w-auto text-xs"
						value={toLocalInputValue(customStart)}
						onChange={(e) => onChange({ customStart: parseLocalInput(e.target.value) })}
					/>
					<span className="text-xs text-muted-foreground">~</span>
					<Input
						type="datetime-local"
						step={1}
						data-testid="custom-end"
						className="h-8 w-auto text-xs"
						value={toLocalInputValue(customEnd)}
						onChange={(e) => onChange({ customEnd: parseLocalInput(e.target.value) })}
					/>
					<Button
						variant="outline"
						size="sm"
						className="h-8 text-xs"
						onClick={() => {
							if (customEnd > customStart) {
								onChange({ appliedCustom: { startTime: customStart, endTime: customEnd } });
							}
						}}
					>
						应用
					</Button>
				</div>
			)}
		</div>
	);
}

/** 由 RaceWindowState 派生查询窗口（毫秒起止）。 */
export function raceWindowBounds(
	state: RaceWindowState,
	now: number,
): { startTime: number; endTime: number } {
	if (state.period === "custom") {
		return state.appliedCustom ?? { startTime: state.customStart, endTime: state.customEnd };
	}
	return periodBounds(state.period, state.offset, now);
}
