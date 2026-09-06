import { MidEllipsis } from "@/components/mid-ellipsis";
import {
	RaceWindowControl,
	type RaceWindowState,
	defaultRaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useInView } from "@/hooks/use-in-view";
import { formatPeriodLabel } from "@/lib/race-period";
import type { RaceWindow } from "@/lib/race-types";
import type { LucideIcon } from "lucide-react";
import { type ReactNode, type RefObject, useState } from "react";
import { useTranslation } from "react-i18next";

/** 赛马卡窗口视图：固化 now + 窗口状态 + 懒加载观察（四张赛马卡共用）。 */
export interface RaceCardWindow {
	/** 挂载时刻固化的 now（当前周期标题/窗口终点稳定）。 */
	now: number;
	windowState: RaceWindowState;
	onWindowChange: (patch: Partial<RaceWindowState>) => void;
	/** 由窗口状态派生的查询窗口（毫秒起止）。 */
	window: RaceWindow;
	/** 挂到 Card 上的懒加载观察 ref。 */
	ref: RefObject<HTMLDivElement | null>;
	inView: boolean;
}

/** 赛马卡窗口状态机：now 固化、窗口状态、派生窗口与懒加载。 */
export function useRaceCardWindow(initialWindow?: RaceWindowState): RaceCardWindow {
	// 挂载时刻固化 now：保证「当前周期」的窗口终点稳定，不因渲染抖动重复请求。
	const [now] = useState(() => Date.now());
	const [windowState, setWindowState] = useState<RaceWindowState>(
		() => initialWindow ?? defaultRaceWindowState(),
	);
	const window = raceWindowBounds(windowState, now);
	const { ref, inView } = useInView();
	const onWindowChange = (patch: Partial<RaceWindowState>) => {
		setWindowState((prev) => ({ ...prev, ...patch }));
	};
	return { now, windowState, onWindowChange, window, ref, inView };
}

interface RaceCardShellProps {
	view: RaceCardWindow;
	icon: LucideIcon;
	/** 卡片标题（i18n key）。 */
	titleKey: string;
	/** 查询状态：壳层据此渲染骨架/错误重试/无数据/内容。 */
	status: {
		isLoading: boolean;
		isError: boolean;
		/** 数据存在但无条目。 */
		isEmpty: boolean;
		onRetry: () => void;
	};
	children: ReactNode;
}

/**
 * 赛马卡壳层：图标 + 标题 + 窗口副标题 + 窗口控件的卡片头，以及
 * 「未进视口 → 骨架 → 错误重试 → 无数据 → 内容」状态分支。
 * 四张赛马卡（供应商/虚拟模型/供应商模型/API Key）共用；内容与查询由调用方注入。
 */
export function RaceCardShell({
	view,
	icon: Icon,
	titleKey,
	status,
	children,
}: RaceCardShellProps) {
	const { t } = useTranslation();
	const { now, windowState, onWindowChange, ref, inView } = view;
	return (
		<Card ref={ref} className="p-5">
			<div className="mb-4 flex flex-wrap items-center gap-3">
				<span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
					<Icon className="h-4 w-4" />
				</span>
				<div className="min-w-0">
					<h3 className="text-sm font-semibold text-foreground">{t(titleKey)}</h3>
					<MidEllipsis
						className="text-xs text-muted-foreground"
						text={
							windowState.period === "custom"
								? t("overview.customWindow")
								: formatPeriodLabel(windowState.period, windowState.offset, now)
						}
					/>
				</div>

				<div className="ml-auto">
					<RaceWindowControl state={windowState} now={now} onChange={onWindowChange} />
				</div>
			</div>

			{!inView ? (
				<div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">
					{t("race.loadingAfterScroll")}
				</div>
			) : status.isLoading ? (
				<Skeleton className="h-[220px] rounded-lg" />
			) : status.isError ? (
				<div className="flex h-[220px] flex-col items-center justify-center gap-2 text-xs text-muted-foreground">
					<span>{t("race.loadFailed")}</span>
					<button
						type="button"
						className="rounded-full bg-foreground/5 px-3 py-1 text-xs font-medium hover:bg-foreground/10"
						onClick={status.onRetry}
					>
						{t("common.retry")}
					</button>
				</div>
			) : status.isEmpty ? (
				<div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">
					{t("race.noData")}
				</div>
			) : (
				children
			)}
		</Card>
	);
}
