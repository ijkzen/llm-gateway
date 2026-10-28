import { MidEllipsis } from "@/components/mid-ellipsis";
import { Badge } from "@/components/ui/badge";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import type { SpeedTestResultRow } from "@/hooks/use-speed-test";
import { cn } from "@/lib/utils";
import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

/** 可排序列。`tps` 默认降序（快的在前），其余默认降序，失败行始终沉底。 */
type SortKey = "providerName" | "modelId" | "tps" | "ttftMs" | "outputTokens" | "totalMs";
type SortOrder = "asc" | "desc";

interface SpeedTestTableProps {
	rows: SpeedTestResultRow[];
	/** 输出 token 低于此值时标「样本不足」（TPS 可信度提示）。 */
	lowSampleThreshold?: number;
}

/** 样本不足阈值：低于它时 TPS 仍展示，但标注可信度（ADR-0027）。 */
const DEFAULT_LOW_SAMPLE_THRESHOLD = 128;

function formatTps(value?: number | null): string {
	return value === undefined || value === null ? "—" : value.toFixed(2);
}

function formatMs(value?: number | null): string {
	return value === undefined || value === null ? "—" : `${value.toFixed(0)} ms`;
}

function formatTime(ms?: number | null): string {
	if (ms === undefined || ms === null) return "—";
	return new Date(ms).toLocaleTimeString();
}

/** 排序值：缺值排最后（无论升降序），失败行由外层二次沉底。 */
function sortValue(row: SpeedTestResultRow, key: SortKey): number | string | null {
	switch (key) {
		case "providerName":
			return row.providerName;
		case "modelId":
			return row.modelId;
		case "tps":
			return row.tps ?? null;
		case "ttftMs":
			return row.ttftMs ?? null;
		case "outputTokens":
			return row.outputTokens ?? null;
		case "totalMs":
			return row.totalMs ?? null;
	}
}

/**
 * 结果表：不分页、无单行重测（要重测就整批再跑一次）。
 *
 * 排序规则两层：先按「有成绩的行在前、失败/取消/未开始在后」分区，再在分区内
 * 按选定列排序。这样点任何列头，失败行都不会跑到上面把成功成绩挤下去。
 */
export function SpeedTestTable({
	rows,
	lowSampleThreshold = DEFAULT_LOW_SAMPLE_THRESHOLD,
}: SpeedTestTableProps) {
	const { t } = useTranslation();
	const [sortKey, setSortKey] = useState<SortKey>("tps");
	const [sortOrder, setSortOrder] = useState<SortOrder>("desc");

	const sorted = useMemo(() => {
		const ranked = rows.filter((row) => row.status === "done");
		const rest = rows.filter((row) => row.status !== "done");
		const compare = (a: SpeedTestResultRow, b: SpeedTestResultRow) => {
			const left = sortValue(a, sortKey);
			const right = sortValue(b, sortKey);
			if (left === null && right === null) return 0;
			if (left === null) return 1;
			if (right === null) return -1;
			const base =
				typeof left === "string" && typeof right === "string"
					? left.localeCompare(right)
					: Number(left) - Number(right);
			return sortOrder === "asc" ? base : -base;
		};
		return [...ranked.sort(compare), ...rest];
	}, [rows, sortKey, sortOrder]);

	const toggleSort = (key: SortKey) => {
		if (key === sortKey) {
			setSortOrder((prev) => (prev === "asc" ? "desc" : "asc"));
			return;
		}
		setSortKey(key);
		// 耗时类与 TPS 一样默认降序（先看最大的），文本列默认升序更自然。
		setSortOrder(key === "providerName" || key === "modelId" ? "asc" : "desc");
	};

	const columns: Array<{ key: SortKey; label: string; className?: string }> = [
		{ key: "providerName", label: t("speedTest.columnProvider") },
		{ key: "modelId", label: t("speedTest.columnModel") },
	];

	return (
		<div className="overflow-x-auto">
			<Table>
				<TableHeader>
					<TableRow>
						{columns.map((column) => (
							<TableHead key={column.key}>
								<SortButton
									label={column.label}
									active={sortKey === column.key}
									order={sortOrder}
									onClick={() => toggleSort(column.key)}
								/>
							</TableHead>
						))}
						<TableHead>{t("speedTest.columnStatus")}</TableHead>
						{[
							{ key: "tps" as SortKey, label: t("speedTest.columnTps") },
							{ key: "ttftMs" as SortKey, label: t("speedTest.columnTtft") },
							{ key: "outputTokens" as SortKey, label: t("speedTest.columnOutputTokens") },
							{ key: "totalMs" as SortKey, label: t("speedTest.columnTotalMs") },
						].map((column) => (
							<TableHead key={column.key}>
								<SortButton
									label={column.label}
									active={sortKey === column.key}
									order={sortOrder}
									onClick={() => toggleSort(column.key)}
								/>
							</TableHead>
						))}
						<TableHead>{t("speedTest.columnFinishedAt")}</TableHead>
						<TableHead>{t("speedTest.columnFailReason")}</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{sorted.map((row) => {
						const lowSample =
							row.status === "done" &&
							row.outputTokens !== null &&
							row.outputTokens !== undefined &&
							row.outputTokens < lowSampleThreshold;
						return (
							<TableRow key={`${row.providerId}-${row.providerModelPk}`}>
								<TableCell className="max-w-[140px]">
									<MidEllipsis text={row.providerName} />
								</TableCell>
								<TableCell className="max-w-[200px]">
									<MidEllipsis text={row.modelId} />
								</TableCell>
								<TableCell>
									<StatusBadge status={row.status} />
								</TableCell>
								<TableCell className="tabular-nums">
									{formatTps(row.tps)}
									{lowSample ? (
										<Badge variant="outline" className="ml-2">
											{t("speedTest.lowSample")}
										</Badge>
									) : null}
								</TableCell>
								<TableCell className="tabular-nums">{formatMs(row.ttftMs)}</TableCell>
								<TableCell className="tabular-nums">
									{row.outputTokens === null || row.outputTokens === undefined
										? "—"
										: row.outputTokens}
								</TableCell>
								<TableCell className="tabular-nums">{formatMs(row.totalMs)}</TableCell>
								<TableCell className="tabular-nums">{formatTime(row.finishedAt)}</TableCell>
								<TableCell className="max-w-[220px]">
									{row.failReason ? (
										<div className="flex items-center gap-2">
											<MidEllipsis text={row.failReason} />
											{row.requestId ? (
												<Link
													to={`/request-logs?requestId=${encodeURIComponent(row.requestId)}`}
													className="shrink-0 text-xs text-primary underline-offset-4 hover:underline"
												>
													{t("speedTest.viewRequestLog")}
												</Link>
											) : null}
										</div>
									) : row.requestId ? (
										<Link
											to={`/request-logs?requestId=${encodeURIComponent(row.requestId)}`}
											className="text-xs text-primary underline-offset-4 hover:underline"
										>
											{t("speedTest.viewRequestLog")}
										</Link>
									) : (
										"—"
									)}
								</TableCell>
							</TableRow>
						);
					})}
				</TableBody>
			</Table>
		</div>
	);
}

function SortButton({
	label,
	active,
	order,
	onClick,
}: {
	label: string;
	active: boolean;
	order: SortOrder;
	onClick: () => void;
}) {
	const Icon = active ? (order === "asc" ? ArrowUp : ArrowDown) : ChevronsUpDown;
	return (
		<button
			type="button"
			onClick={onClick}
			className={cn(
				"-ml-1 inline-flex items-center gap-1 rounded px-1 py-0.5 hover:text-foreground",
				active ? "text-foreground" : "text-muted-foreground",
			)}
		>
			{label}
			<Icon className="size-3" />
		</button>
	);
}

function StatusBadge({ status }: { status: SpeedTestResultRow["status"] }) {
	const { t } = useTranslation();
	const variant =
		status === "done"
			? "default"
			: status === "failed"
				? "destructive"
				: status === "cancelled"
					? "secondary"
					: "outline";
	return <Badge variant={variant}>{t(`speedTest.status.${status}`)}</Badge>;
}
