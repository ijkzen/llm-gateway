import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTableViewOptions } from "@/components/data-table/view-options";
import { EmptyState } from "@/components/empty-state";
import { RequestLogDetailDialog } from "@/components/request-logs/RequestLogDetailDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { useApiKeys } from "@/hooks/use-api-keys";
import { type RequestLogRow, useRequestLogs } from "@/hooks/use-request-logs";
import { useVirtualModels } from "@/hooks/use-virtual-models";
import {
	type ColumnDef,
	type SortingState,
	type VisibilityState,
	flexRender,
	getCoreRowModel,
	useReactTable,
} from "@tanstack/react-table";
import { RotateCcw } from "lucide-react";
import { useMemo, useState } from "react";

const PLAIN_HEADER_CLASS = "text-xs font-medium uppercase tracking-wider text-muted-foreground";

/** 快捷时间段选项。 */
const QUICK_RANGES = [
	{ value: "1h", label: "最近 1 小时", ms: 60 * 60 * 1000 },
	{ value: "24h", label: "最近 24 小时", ms: 24 * 60 * 60 * 1000 },
	{ value: "7d", label: "最近 7 天", ms: 7 * 24 * 60 * 60 * 1000 },
	{ value: "all", label: "全部", ms: null },
] as const;

function fmtTime(ms: number): string {
	return new Date(ms).toLocaleString("zh-CN", { hour12: false });
}

function fmtTokens(v: number | null | undefined): string {
	if (v === null || v === undefined) return "—";
	return v.toLocaleString("zh-CN");
}

function fmtMs(v: number | null | undefined): string {
	if (v === null || v === undefined) return "—";
	return `${v} ms`;
}

export function RequestLogsTable() {
	// 默认按时间降序（最新在前），列头显示排序指示，亦显式通知后端。
	const [sorting, setSorting] = useState<SortingState>([{ id: "startTime", desc: true }]);
	const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
	const [page, setPage] = useState(1);
	const [pageSize, setPageSize] = useState(20);
	const [detailRow, setDetailRow] = useState<RequestLogRow | null>(null);

	// 过滤条件。
	const [vmId, setVmId] = useState<number | undefined>(undefined);
	const [apiKey, setApiKey] = useState<string | undefined>(undefined);
	const [startTime, setStartTime] = useState<number | undefined>(undefined);
	const [endTime, setEndTime] = useState<number | undefined>(undefined);
	const [quickRange, setQuickRange] = useState<string>("24h");
	const [customStart, setCustomStart] = useState("");
	const [customEnd, setCustomEnd] = useState("");

	const { data: virtualModels } = useVirtualModels();
	const { data: apiKeys } = useApiKeys();

	const sortBy = sorting[0]?.id;
	const sortOrder = sorting[0]?.desc ? "desc" : "asc";

	const query = useRequestLogs({
		page,
		pageSize,
		vmId,
		apiKey,
		startTime,
		endTime,
		sortBy,
		sortOrder,
	});
	const { data, isLoading, isError, refetch } = query;

	const applyQuickRange = (value: string) => {
		setQuickRange(value);
		const range = QUICK_RANGES.find((r) => r.value === value);
		setStartTime(range?.ms ? Date.now() - range.ms : undefined);
		setEndTime(undefined);
		setPage(1);
	};

	const applyCustomRange = () => {
		setStartTime(customStart ? new Date(customStart).getTime() : undefined);
		setEndTime(customEnd ? new Date(customEnd).getTime() : undefined);
		setQuickRange("all");
		setPage(1);
	};

	const resetAll = () => {
		setQuickRange("24h");
		setCustomStart("");
		setCustomEnd("");
		setStartTime(Date.now() - 24 * 60 * 60 * 1000);
		setEndTime(undefined);
		setVmId(undefined);
		setApiKey(undefined);
		setPage(1);
	};

	const columns = useMemo<ColumnDef<RequestLogRow>[]>(
		() => [
			{
				accessorKey: "virtualModelDisplayId",
				meta: { title: "虚拟模型" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="虚拟模型" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => (
					<span className="font-medium">
						{row.original.virtualModelDisplayId ?? `#${row.original.virtualModelId}`}
					</span>
				),
			},
			{
				accessorKey: "apiKeyName",
				meta: { title: "API Key" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="API Key" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => <span className="font-mono">{row.getValue("apiKeyName")}</span>,
			},
			{
				accessorKey: "modelId",
				meta: { title: "上游模型" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="上游模型" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => <span className="font-mono">{row.getValue("modelId")}</span>,
			},
			{
				accessorKey: "success",
				meta: { title: "结果" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="结果" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => {
					const ok = row.original.success;
					return <Badge variant={ok ? "default" : "destructive"}>{ok ? "成功" : "失败"}</Badge>;
				},
			},
			{
				accessorKey: "inputTokens",
				meta: { title: "输入 Token" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="输入" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => fmtTokens(row.original.inputTokens),
			},
			{
				accessorKey: "outputTokens",
				meta: { title: "输出 Token" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="输出" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => fmtTokens(row.original.outputTokens),
			},
			{
				accessorKey: "requestTime",
				meta: { title: "总耗时" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="耗时" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => fmtMs(row.original.requestTime),
			},
			{
				accessorKey: "startTime",
				meta: { title: "时间" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="时间" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => fmtTime(row.original.startTime),
			},
		],
		[],
	);

	const table = useReactTable({
		data: data?.items ?? [],
		columns,
		state: { sorting, columnVisibility },
		onSortingChange: (updater) => {
			setPage(1);
			setSorting(updater);
		},
		onColumnVisibilityChange: setColumnVisibility,
		getCoreRowModel: getCoreRowModel(),
		manualSorting: true,
	});

	const totalPages = Math.max(1, Math.ceil((data?.total ?? 0) / pageSize));

	return (
		<div className="space-y-4">
			{/* 过滤工具栏 */}
			<div className="flex flex-wrap items-end gap-3 rounded-2xl border border-white/70 bg-white/65 p-3 shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04]">
				<div className="space-y-1">
					<p className="text-xs text-muted-foreground">时间段</p>
					<Select value={quickRange} onValueChange={applyQuickRange}>
						<SelectTrigger className="w-[140px]" aria-label="快捷时间段">
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							{QUICK_RANGES.map((r) => (
								<SelectItem key={r.value} value={r.value}>
									{r.label}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
				</div>
				<div className="space-y-1">
					<p className="text-xs text-muted-foreground">自定义起止</p>
					<div className="flex items-center gap-2">
						<Input
							type="datetime-local"
							className="h-9 w-auto"
							value={customStart}
							onChange={(e) => setCustomStart(e.target.value)}
						/>
						<span className="text-xs text-muted-foreground">至</span>
						<Input
							type="datetime-local"
							className="h-9 w-auto"
							value={customEnd}
							onChange={(e) => setCustomEnd(e.target.value)}
						/>
						<Button type="button" variant="outline" size="sm" onClick={applyCustomRange}>
							应用
						</Button>
					</div>
				</div>
				<div className="space-y-1">
					<p className="text-xs text-muted-foreground">虚拟模型</p>
					<Select
						value={vmId !== undefined ? String(vmId) : "all"}
						onValueChange={(v) => {
							setVmId(v === "all" ? undefined : Number(v));
							setPage(1);
						}}
					>
						<SelectTrigger className="w-[160px]" aria-label="按虚拟模型过滤">
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">全部</SelectItem>
							{virtualModels?.map((vm) => (
								<SelectItem key={vm.virtualModelId} value={String(vm.virtualModelId)}>
									{vm.displayId}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
				</div>
				<div className="space-y-1">
					<p className="text-xs text-muted-foreground">API Key</p>
					<Select
						value={apiKey ?? "all"}
						onValueChange={(v) => {
							setApiKey(v === "all" ? undefined : v);
							setPage(1);
						}}
					>
						<SelectTrigger className="w-[160px]" aria-label="按 API Key 过滤">
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">全部</SelectItem>
							{apiKeys?.map((key) => (
								<SelectItem key={key.id} value={key.name}>
									{key.name}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
				</div>
				<Button type="button" variant="outline" size="sm" onClick={resetAll} className="ml-auto">
					<RotateCcw className="mr-1.5 size-4" />
					重置
				</Button>
			</div>

			<div className="flex justify-end">
				<DataTableViewOptions table={table} />
			</div>

			{isError ? (
				<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-destructive">
					加载失败
					<Button variant="outline" size="sm" className="ml-3" onClick={() => refetch()}>
						重试
					</Button>
				</div>
			) : (
				<div className="overflow-x-auto rounded-2xl border border-white/70 bg-white/65 shadow-[0_4px_16px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04] dark:shadow-[0_10px_24px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.06)]">
					<Table>
						<TableHeader>
							{table.getHeaderGroups().map((headerGroup) => (
								<TableRow key={headerGroup.id} className="hover:bg-transparent">
									{headerGroup.headers.map((header) => (
										<TableHead key={header.id}>
											{header.isPlaceholder
												? null
												: flexRender(header.column.columnDef.header, header.getContext())}
										</TableHead>
									))}
								</TableRow>
							))}
						</TableHeader>
						<TableBody>
							{isLoading ? (
								<TableRow>
									<TableCell
										colSpan={columns.length}
										className="py-10 text-center text-muted-foreground"
									>
										加载中...
									</TableCell>
								</TableRow>
							) : data && data.items.length > 0 ? (
								table.getRowModel().rows.map((row) => (
									<TableRow
										key={row.id}
										className="cursor-pointer transition-colors hover:bg-muted/50"
										onClick={() => setDetailRow(row.original)}
									>
										{row.getVisibleCells().map((cell) => (
											<TableCell key={cell.id}>
												{flexRender(cell.column.columnDef.cell, cell.getContext())}
											</TableCell>
										))}
									</TableRow>
								))
							) : (
								<TableRow>
									<TableCell colSpan={columns.length} className="py-0">
										<EmptyState
											title="暂无请求日志"
											description="调整过滤条件或等待新的转发请求"
											className="border-0 bg-transparent shadow-none"
										/>
									</TableCell>
								</TableRow>
							)}
						</TableBody>
					</Table>
				</div>
			)}

			{/* 服务端分页 */}
			<div className="flex flex-wrap items-center justify-between gap-4">
				<p className="text-sm text-muted-foreground">
					共 {data?.total ?? 0} 条 · 第 {page} / {totalPages} 页
				</p>
				<div className="flex items-center gap-2">
					<Select
						value={String(pageSize)}
						onValueChange={(v) => {
							setPageSize(Number(v));
							setPage(1);
						}}
					>
						<SelectTrigger className="h-8 w-[110px]" aria-label="每页条数">
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							{[10, 20, 50, 100].map((size) => (
								<SelectItem key={size} value={String(size)}>
									{size} / 页
								</SelectItem>
							))}
						</SelectContent>
					</Select>
					<Button
						variant="outline"
						size="sm"
						disabled={page <= 1}
						onClick={() => setPage((p) => Math.max(1, p - 1))}
					>
						上一页
					</Button>
					<Button
						variant="outline"
						size="sm"
						disabled={page >= totalPages}
						onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
					>
						下一页
					</Button>
				</div>
			</div>

			<RequestLogDetailDialog
				row={detailRow}
				onOpenChange={(open) => !open && setDetailRow(null)}
			/>
		</div>
	);
}
