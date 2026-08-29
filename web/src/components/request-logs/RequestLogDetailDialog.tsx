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
	// 供应商名称通过详情接口按 providerId 查询（request 表不存名称，不新增字段）。
	const { data: provider } = useProviderDetail(row?.providerId ?? null);
	const providerLabel = provider?.name ?? (row ? `#${row.providerId}` : "");

	return (
		<Dialog open={row !== null} onOpenChange={onOpenChange}>
			<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[640px]">
				<DialogHeader className="space-y-3">
					<DialogTitle className="font-mono text-sm">请求 {row?.requestId}</DialogTitle>
					<DialogDescription>请求日志完整字段；点击任意值可选中复制</DialogDescription>
				</DialogHeader>
				{row && (
					<div className="space-y-5">
						{/* 概览徽章 */}
						<div className="flex flex-wrap items-center gap-2">
							<Badge variant={row.success ? "default" : "destructive"}>
								{row.success ? "成功" : "失败"}
							</Badge>
							{row.stream && <Badge variant="outline">流式</Badge>}
							{row.virtualModelDisplayId && (
								<Badge variant="secondary">{row.virtualModelDisplayId}</Badge>
							)}
							{provider && <Badge variant="secondary">{provider.name}</Badge>}
						</div>

						<div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
							<Field label="虚拟模型 ID">
								{fmt(row.virtualModelDisplayId ?? row.virtualModelId)}
							</Field>
							<Field label="API Key">{fmt(row.apiKeyName)}</Field>
							<Field label="供应商">{fmt(providerLabel)}</Field>
							<Field label="上游模型">{fmt(row.modelId)}</Field>
							<Field label="开始时间">{fmtTime(row.startTime)}</Field>
							<Field label="结束时间">{fmtTime(row.endTime)}</Field>
							<Field label="总耗时">{fmtMs(row.requestTime)}</Field>
							<Field label="网络延迟">{fmtMs(row.networkLatency)}</Field>
							<Field label="首 token 耗时 (TTFT)">{fmtMs(row.ttft)}</Field>
							<Field label="输出耗时">{fmtMs(row.outputTokensTime)}</Field>
							<Field label="输入 Token">
								{row.inputTokens === null || row.inputTokens === undefined
									? "—"
									: row.inputTokens.toLocaleString("zh-CN")}
							</Field>
							<Field label="输出 Token">
								{row.outputTokens === null || row.outputTokens === undefined
									? "—"
									: row.outputTokens.toLocaleString("zh-CN")}
							</Field>
							<Field label="总 Token">
								{row.totalTokens === null || row.totalTokens === undefined
									? "—"
									: row.totalTokens.toLocaleString("zh-CN")}
							</Field>
							<Field label="缓存命中 Token">{row.inputCacheTokens.toLocaleString("zh-CN")}</Field>
							<Field label="缓存命中率">{(row.inputCacheRate * 100).toFixed(1)}%</Field>
							<Field label="TPS">{row.tps.toFixed(2)}</Field>
							<Field label="请求 ID">{fmt(row.requestId)}</Field>
						</div>

						{row.failReason && (
							<div>
								<p className="mb-1 text-xs font-medium uppercase tracking-wider text-muted-foreground">
									失败原因
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
