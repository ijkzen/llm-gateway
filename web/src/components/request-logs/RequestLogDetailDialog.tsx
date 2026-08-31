import { Badge } from "@/components/ui/badge";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { useProviderDetail } from "@/hooks/use-providers";
import type { RequestLogRow } from "@/hooks/use-request-logs";
import { formatPercent } from "@/lib/utils";
import { useTranslation } from "react-i18next";

interface RequestLogDetailDialogProps {
	row: RequestLogRow | null;
	onOpenChange: (open: boolean) => void;
}

function fmt(value: unknown): string {
	if (value === null || value === undefined || value === "") return "—";
	if (typeof value === "number") return String(value);
	return String(value);
}

function fmtMs(ms: number | null | undefined): string {
	if (ms === null || ms === undefined) return "—";
	return `${ms.toLocaleString("zh-CN")} ms`;
}

function fmtTime(ms: number | null | undefined): string {
	if (ms === null || ms === undefined) return "—";
	const d = new Date(ms);
	return `${d.toLocaleDateString("zh-CN")} ${d.toLocaleTimeString("zh-CN")}`;
}

/** 详情字段行：label + 值，值可选中复制。 */
function Field({ label, children }: { label: string; children: React.ReactNode }) {
	return (
		<div className="space-y-1">
			<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">{label}</p>
			<div className="min-w-0 break-all rounded-md bg-muted/40 px-2.5 py-1.5 font-mono text-sm">
				{children}
			</div>
		</div>
	);
}

/** 请求日志详情弹窗：点击表格行后展示该请求的全部字段。 */
export function RequestLogDetailDialog({ row, onOpenChange }: RequestLogDetailDialogProps) {
	const { t } = useTranslation();
	// 供应商名称通过详情接口按 providerId 查询（request 表不存名称，不新增字段）。
	const { data: provider } = useProviderDetail(row?.providerId ?? null);
	const providerLabel = provider?.name ?? (row ? `#${row.providerId}` : "");

	return (
		<Dialog open={row !== null} onOpenChange={onOpenChange}>
			<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[640px]">
				<DialogHeader className="space-y-3">
					<DialogTitle className="font-mono text-sm">
						{t("requestLogs.requestTitle")} {row?.requestId}
					</DialogTitle>
					<DialogDescription>{t("requestLogs.fullFieldsHint")}</DialogDescription>
				</DialogHeader>
				{row && (
					<div className="space-y-5">
						{/* 概览徽章 */}
						<div className="flex flex-wrap items-center gap-2">
							<Badge variant={row.success ? "default" : "destructive"}>
								{row.success ? t("requestLogs.success") : t("requestLogs.failed")}
							</Badge>
							{row.stream && <Badge variant="outline">{t("requestLogs.stream")}</Badge>}
							{row.virtualModelDisplayId && (
								<Badge variant="secondary">{row.virtualModelDisplayId}</Badge>
							)}
							{provider && <Badge variant="secondary">{provider.name}</Badge>}
						</div>

						<div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
							<Field label={t("virtualModels.displayId")}>
								{fmt(row.virtualModelDisplayId ?? row.virtualModelId)}
							</Field>
							<Field label={t("requestLogs.apiKey")}>{fmt(row.apiKeyName)}</Field>
							<Field label={t("requestLogs.provider")}>{fmt(providerLabel)}</Field>
							<Field label={t("requestLogs.upstreamModel")}>{fmt(row.modelId)}</Field>
							<Field label={t("cronJobs.startedAt")}>{fmtTime(row.startTime)}</Field>
							<Field label={t("cronJobs.endedAt")}>{fmtTime(row.endTime)}</Field>
							<Field label={t("requestLogs.totalTime")}>{fmtMs(row.requestTime)}</Field>
							<Field label={t("requestLogs.ttftDetail")}>{fmtMs(row.ttft)}</Field>
							<Field label={t("requestLogs.outputTime")}>{fmtMs(row.outputTokensTime)}</Field>
							<Field label={t("requestLogs.inputTokens")}>
								{row.inputTokens === null || row.inputTokens === undefined
									? "—"
									: row.inputTokens.toLocaleString("zh-CN")}
							</Field>
							<Field label={t("requestLogs.outputTokens")}>
								{row.outputTokens === null || row.outputTokens === undefined
									? "—"
									: row.outputTokens.toLocaleString("zh-CN")}
							</Field>
							<Field label={t("requestLogs.totalTokens")}>
								{row.totalTokens === null || row.totalTokens === undefined
									? "—"
									: row.totalTokens.toLocaleString("zh-CN")}
							</Field>
							<Field label={t("requestLogs.cacheHitTokens")}>
								{row.inputCacheTokens.toLocaleString("zh-CN")}
							</Field>
							<Field label={t("requestLogs.cacheHitRate")}>
								{formatPercent(row.inputCacheRate)}
							</Field>
							<Field label={t("requestLogs.tps")}>{row.tps.toFixed(2)}</Field>
							<Field label={t("requestLogs.requestId")}>{fmt(row.requestId)}</Field>
						</div>

						{row.failReason && (
							<div>
								<p className="mb-1 text-xs font-medium uppercase tracking-wider text-muted-foreground">
									{t("requestLogs.failReason")}
								</p>
								<div className="break-all rounded-md bg-destructive/10 px-2.5 py-1.5 font-mono text-sm text-destructive">
									{row.failReason}
								</div>
							</div>
						)}
					</div>
				)}
			</DialogContent>
		</Dialog>
	);
}
