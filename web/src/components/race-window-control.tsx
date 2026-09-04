import { SegmentedControl } from "@/components/segmented-control";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
	type RacePeriod,
	defaultCustomWindow,
	formatCompactPeriodLabel,
	formatDateTimeLabel,
	periodBounds,
	toLocalInputValue,
} from "@/lib/race-period";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

const PERIOD_OPTION_VALUES = ["day", "week", "month", "year", "custom"] as const;

const PERIOD_OPTION_KEYS: Record<(typeof PERIOD_OPTION_VALUES)[number], string> = {
	day: "race.day",
	week: "race.week",
	month: "race.month",
	year: "race.year",
	custom: "race.custom",
};

export interface RaceWindowState {
	period: RacePeriod | "custom";
	offset: number;
	customStart: number;
	customEnd: number;
	/** 已应用的自定义窗口（null 时退化为输入值）。 */
	appliedCustom: { startTime: number; endTime: number } | null;
}

/** 从 URL query 解析初始时间段（缺省当天）；列表页/赛马行跳转时携带。 */
export function initialWindowFromUrl(searchParams: URLSearchParams): RaceWindowState {
	const period = (searchParams.get("period") as RacePeriod | "custom" | null) ?? "day";
	const offset = Number.parseInt(searchParams.get("offset") ?? "0", 10) || 0;
	const now = Date.now();
	const startTime = Number(searchParams.get("startTime")) || now - 3_600_000;
	const endTime = Number(searchParams.get("endTime")) || now;
	return {
		period,
		offset,
		customStart: startTime,
		customEnd: endTime,
		appliedCustom: period === "custom" ? { startTime, endTime } : null,
	};
}

interface RaceWindowControlProps {
	state: RaceWindowState;
	/** 固化 now（当前周期标题用）。 */
	now: number;
	onChange: (patch: Partial<RaceWindowState>) => void;
	/** 是否显示当前窗口标题（默认 true）。 */
	showLabel?: boolean;
}

/** datetime-local 值 → 毫秒时间戳；空值返回当前时刻（避免 NaN）。 */
function parseLocalInput(value: string): number {
	const ms = new Date(value).getTime();
	return Number.isNaN(ms) ? Date.now() : ms;
}

/**
 * 赛马时间窗口共享控件：天/周/月/年 SegmentedControl + 左右箭头切换周期 +
 * 自定义两行起止文本（点击弹窗选时间）。首页与二级/三级页共用。
 */
export function RaceWindowControl({
	state,
	now,
	onChange,
	showLabel = true,
}: RaceWindowControlProps) {
	const { t } = useTranslation();
	const { period, offset, customStart, customEnd } = state;
	const [dialogOpen, setDialogOpen] = useState(false);
	const [draftStart, setDraftStart] = useState(customStart);
	const [draftEnd, setDraftEnd] = useState(customEnd);
	const periodOptions = PERIOD_OPTION_VALUES.map((value) => ({
		value,
		label: t(PERIOD_OPTION_KEYS[value]),
	}));

	const changePeriod = (next: RacePeriod | "custom") => {
		onChange({ period: next });
		if (next === "custom" && !state.appliedCustom) {
			// 首次进入自定义：默认「过去 7 天」（7 天前 0 点 ~ 明天 0 点）并立即生效。
			const defaults = defaultCustomWindow(now);
			onChange({
				customStart: defaults.startTime,
				customEnd: defaults.endTime,
				appliedCustom: defaults,
			});
		}
	};

	const openDialog = () => {
		// 弹窗草稿以当前已应用（或输入）的起止为准。
		const start = state.appliedCustom?.startTime ?? customStart;
		const end = state.appliedCustom?.endTime ?? customEnd;
		setDraftStart(start);
		setDraftEnd(end);
		setDialogOpen(true);
	};

	const confirmCustom = () => {
		if (draftEnd > draftStart) {
			onChange({
				customStart: draftStart,
				customEnd: draftEnd,
				appliedCustom: { startTime: draftStart, endTime: draftEnd },
			});
		}
		setDialogOpen(false);
	};

	// 展示用的起止（未应用时退化为输入值）。
	const displayStart = state.appliedCustom?.startTime ?? customStart;
	const displayEnd = state.appliedCustom?.endTime ?? customEnd;

	return (
		<div className="flex flex-wrap items-center gap-2">
			<SegmentedControl options={periodOptions} value={period} onChange={changePeriod} />
			{period === "custom" ? (
				showLabel && (
					<button
						type="button"
						onClick={openDialog}
						aria-label={t("race.customRangeLabel")}
						data-testid="custom-range-label"
						className="space-y-0.5 rounded-lg px-2 py-1 text-left text-xs text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground"
					>
						<div className="font-mono tabular-nums">
							{t("race.startLabel")} {formatDateTimeLabel(displayStart)}
						</div>
						<div className="font-mono tabular-nums">
							{t("race.endLabel")} {formatDateTimeLabel(displayEnd)}
						</div>
					</button>
				)
			) : (
				<div className="flex items-center gap-1">
					<Button
						variant="ghost"
						size="icon"
						className="h-6 w-6"
						aria-label={t("race.previous")}
						onClick={() => onChange({ offset: offset - 1 })}
					>
						<ChevronLeft className="h-4 w-4" />
					</Button>
					{showLabel && (
						<span className="min-w-24 text-center text-xs font-medium text-foreground">
							{formatCompactPeriodLabel(period, offset, now)}
						</span>
					)}
					<Button
						variant="ghost"
						size="icon"
						className="h-6 w-6"
						aria-label={t("race.next")}
						onClick={() => onChange({ offset: offset + 1 })}
					>
						<ChevronRight className="h-4 w-4" />
					</Button>
				</div>
			)}

			<Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
				<DialogContent className="sm:max-w-sm">
					<DialogHeader>
						<DialogTitle>{t("race.customRange")}</DialogTitle>
					</DialogHeader>
					<div className="space-y-4 py-2">
						<div className="space-y-1.5">
							<p className="text-xs text-muted-foreground">{t("race.startTime")}</p>
							<Input
								type="datetime-local"
								step={1}
								data-testid="custom-start"
								className="h-9 w-full"
								value={toLocalInputValue(draftStart)}
								onChange={(e) => setDraftStart(parseLocalInput(e.target.value))}
							/>
						</div>
						<div className="space-y-1.5">
							<p className="text-xs text-muted-foreground">{t("race.endTime")}</p>
							<Input
								type="datetime-local"
								step={1}
								data-testid="custom-end"
								className="h-9 w-full"
								value={toLocalInputValue(draftEnd)}
								onChange={(e) => setDraftEnd(parseLocalInput(e.target.value))}
							/>
						</div>
						{draftEnd <= draftStart && (
							<p className="text-xs text-destructive">{t("race.timeInvalid")}</p>
						)}
					</div>
					<DialogFooter>
						<Button variant="outline" size="sm" onClick={() => setDialogOpen(false)}>
							{t("common.cancel")}
						</Button>
						<Button size="sm" onClick={confirmCustom}>
							{t("common.confirm")}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
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
