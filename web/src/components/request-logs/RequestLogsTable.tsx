import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTableViewOptions } from "@/components/data-table/view-options";
import { EmptyState } from "@/components/empty-state";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { RequestLogDetailDialog } from "@/components/request-logs/RequestLogDetailDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { useProviderModels } from "@/hooks/use-provider-models";
import { useProviders } from "@/hooks/use-providers";
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

/** 默认时间窗口状态：今天（自然日 [0点, 当前]）。 */
function defaultTimeWindow(): RaceWindowState {
	const now = Date.now();
	const start = new Date(now);
	start.setHours(0, 0, 0, 0);
	return {
		period: "day",
		offset: 0,
		customStart: start.getTime(),
		customEnd: now,
		appliedCustom: null,
	};
}

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

	// 过滤条件：先过滤（虚拟模型/供应商/供应商模型/结果状态/API Key），再选时间段。
	const [vmId, setVmId] = useState<number | undefined>(undefined);
	const [providerId, setProviderId] = useState<number | undefined>(undefined);
	const [modelId, setModelId] = useState<string | undefined>(undefined);
	const [success, setSuccess] = useState<boolean | undefined>(undefined);
	const [apiKey, setApiKey] = useState<string | undefined>(undefined);
	// 时间过滤：通用时间组件（天/周/月/年/自定义），默认今天。
	const [timeWindow, setTimeWindow] = useState<RaceWindowState>(defaultTimeWindow);
	// now 随重置刷新：当前周期（offset=0）的 endTime 由它派生，
	// 若固化在挂载时刻，重置后结束时间不更新，最新日志查不到。
	const [now, setNow] = useState(() => Date.now());
	const timeBounds = raceWindowBounds(timeWindow, now);

	const { data: virtualModels } = useVirtualModels();
	const { data: providers } = useProviders();
	const { data: allProviderModels } = useProviderModels();
	const { data: apiKeys } = useApiKeys();

	// 供应商模型选项随所选供应商级联过滤。
	const providerModelOptions = useMemo(
		() =>
			(allProviderModels ?? []).filter(
				(model) => providerId === undefined || model.providerId === providerId,
			),
		[allProviderModels, providerId],
	);

	const sortBy = sorting[0]?.id;
	const sortOrder = sorting[0]?.desc ? "desc" : "asc";

	const query = useRequestLogs({
		page,
		pageSize,
		vmId,
		providerId,
		modelId,
		success,
		apiKey,
		startTime: timeBounds.startTime,
		endTime: timeBounds.endTime,
		sortBy,
		sortOrder,
	});
	const { data, isLoading, isError, refetch } = query;

	const resetAll = () => {
		setNow(Date.now());
		setTimeWindow(defaultTimeWindow());
		setVmId(undefined);
		setProviderId(undefined);
		setModelId(undefined);
		setSuccess(undefined);
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
			{/* 过滤卡片：条件（上，重置右对齐）+ 时间（下，显示列右对齐） */}
			<div className="rounded-2xl border border-white/70 bg-white/65 p-3 shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04]">
				<div className="flex flex-wrap items-end gap-3">
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
						<p className="text-xs text-muted-foreground">供应商</p>
						<Select
							value={providerId !== undefined ? String(providerId) : "all"}
							onValueChange={(v) => {
								setProviderId(v === "all" ? undefined : Number(v));
								// 供应商变化后旧模型过滤不再适用，清空并回到全部。
								setModelId(undefined);
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[160px]" aria-label="按供应商过滤">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">全部</SelectItem>
								{providers?.map((p) => (
									<SelectItem key={p.id} value={String(p.id)}>
										{p.name}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</div>
					<div className="space-y-1">
						<p className="text-xs text-muted-foreground">供应商模型</p>
						<Select
							value={modelId ?? "all"}
							onValueChange={(v) => {
								setModelId(v === "all" ? undefined : v);
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[180px]" aria-label="按供应商模型过滤">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">全部</SelectItem>
								{providerModelOptions.map((model) => (
									<SelectItem key={model.modelId} value={model.providerModelId}>
										{model.providerModelId}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</div>
					<div className="space-y-1">
						<p className="text-xs text-muted-foreground">结果</p>
						<Select
							value={success === undefined ? "all" : success ? "true" : "false"}
							onValueChange={(v) => {
								setSuccess(v === "all" ? undefined : v === "true");
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[100px]" aria-label="按结果状态过滤">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">全部</SelectItem>
								<SelectItem value="true">成功</SelectItem>
								<SelectItem value="false">失败</SelectItem>
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
				<div className="mt-3 flex flex-wrap items-center gap-3 border-t border-foreground/5 pt-3">
					<span className="text-xs text-muted-foreground">时间</span>
					<RaceWindowControl
						state={timeWindow}
						now={now}
						onChange={(patch) => setTimeWindow((prev) => ({ ...prev, ...patch }))}
					/>
					<div className="ml-auto">
						<DataTableViewOptions table={table} />
					</div>
				</div>
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
