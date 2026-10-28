import { Badge } from "@/components/ui/badge";
import type { SpeedTestResultRow, SpeedTestRun } from "@/hooks/use-speed-test";
import { useTranslation } from "react-i18next";

interface SpeedTestSummaryProps {
	run: SpeedTestRun | null;
	rows: SpeedTestResultRow[];
}

/**
 * 汇总条：完成时间 · 候选/成功/失败 · 平均 TPS。
 *
 * 平均 TPS 用**算术平均**（不是数据面板的 token 加权）：这里要回答的是「这批
 * 模型平均多快」，与表格主排序一致，用户一眼能对上；加权平均会被单个超快
 * 模型拉高，与表里看到的中间值对不上。
 */
export function SpeedTestSummary({ run, rows }: SpeedTestSummaryProps) {
	const { t } = useTranslation();
	if (!run) return null;

	const done = rows.filter((row) => row.status === "done");
	const failed = rows.filter((row) => row.status === "failed");
	const cancelled = rows.filter((row) => row.status === "cancelled");
	const tpsValues = done
		.map((row) => row.tps)
		.filter((value): value is number => value !== null && value !== undefined);
	const averageTps =
		tpsValues.length > 0
			? tpsValues.reduce((sum, value) => sum + value, 0) / tpsValues.length
			: null;

	const finishedAt = run.finishedAt
		? new Date(run.finishedAt).toLocaleString()
		: t("speedTest.summaryRunning");

	return (
		<div
			className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted-foreground"
			data-testid="speed-test-summary"
		>
			<span>{finishedAt}</span>
			<span aria-hidden="true">·</span>
			<span>
				{t("speedTest.summaryTotal", { count: run.total })} /{" "}
				{t("speedTest.summaryDone", { count: done.length })} /{" "}
				{t("speedTest.summaryFailed", { count: failed.length })}
			</span>
			{cancelled.length > 0 ? (
				<span>{t("speedTest.summaryCancelled", { count: cancelled.length })}</span>
			) : null}
			<span aria-hidden="true">·</span>
			<span>
				{t("speedTest.summaryAverageTps", {
					value: averageTps === null ? "—" : averageTps.toFixed(2),
				})}
			</span>
			<RunStatusBadge status={run.status} />
		</div>
	);
}

function RunStatusBadge({ status }: { status: SpeedTestRun["status"] }) {
	const { t } = useTranslation();
	const variant =
		status === "completed" ? "default" : status === "running" ? "secondary" : "destructive";
	return <Badge variant={variant}>{t(`speedTest.runStatus.${status}`)}</Badge>;
}
