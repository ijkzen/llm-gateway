import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTableViewOptions } from "@/components/data-table/view-options";
import { EmptyState } from "@/components/empty-state";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { RequestLogDetailDialog } from "@/components/request-logs/RequestLogDetailDialog";
import { TableSkeleton } from "@/components/table-skeleton";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
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
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

const PLAIN_HEADER_CLASS = "text-xs font-medium uppercase tracking-wider text-muted-foreground";

/** 列显隐与每页条数的 localStorage 持久化键。 */
const COLUMN_VISIBILITY_KEY = "request-logs:column-visibility";
const PAGE_SIZE_KEY = "request-logs:page-size";
/** 每页条数可选值。 */
const PAGE_SIZE_OPTIONS = [10, 20, 50, 100] as const;
const DEFAULT_PAGE_SIZE = 20;

/** 读 localStorage 的列显隐（JSON 对象）；无数据/解析失败返回空对象（全部列显示）。 */
function loadColumnVisibility(): VisibilityState {
	try {
		const raw = window.localStorage.getItem(COLUMN_VISIBILITY_KEY);
		if (!raw) {
			return {};
		}
		const parsed: unknown = JSON.parse(raw);
		return parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)
			? (parsed as VisibilityState)
			: {};
	} catch {
		return {};
	}
}

/** 读 localStorage 的每页条数；无数据/非法值返回默认 20。 */
function loadPageSize(): number {
	try {
		const raw = window.localStorage.getItem(PAGE_SIZE_KEY);
		const value = Number(raw);
		return (PAGE_SIZE_OPTIONS as readonly number[]).includes(value) ? value : DEFAULT_PAGE_SIZE;
	} catch {
		return DEFAULT_PAGE_SIZE;
	}
}

/** 写 localStorage（隐私模式等写失败时静默忽略）。 */
function saveToStorage(key: string, value: string): void {
	try {
		window.localStorage.setItem(key, value);
	} catch {
		// 忽略：localStorage 不可用时持久化失效但不影响功能。
	}
}

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
	const { t } = useTranslation();
	// 默认按时间降序（最新在前），列头显示排序指示，亦显式通知后端。
	const [sorting, setSorting] = useState<SortingState>([{ id: "startTime", desc: true }]);
	// 列显隐与每页条数从 localStorage 恢复；无数据时默认全部列 + 20 条。
	const [columnVisibility, setColumnVisibility] = useState<VisibilityState>(loadColumnVisibility);
	const [page, setPage] = useState(1);
	const [pageSize, setPageSize] = useState<number>(loadPageSize);
	const [detailRow, setDetailRow] = useState<RequestLogRow | null>(null);

	// 用户调整列显隐 / 每页条数后写回 localStorage，刷新保持。
	useEffect(() => {
		saveToStorage(COLUMN_VISIBILITY_KEY, JSON.stringify(columnVisibility));
	}, [columnVisibility]);
	useEffect(() => {
		saveToStorage(PAGE_SIZE_KEY, String(pageSize));
	}, [pageSize]);

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
				meta: { title: t("requestLogs.virtualModel") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.virtualModel")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => (
					<span className="font-medium">
						{row.original.virtualModelDisplayId ?? `#${row.original.virtualModelId}`}
					</span>
				),
			},
			{
				accessorKey: "apiKeyName",
				meta: { title: t("requestLogs.apiKey") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.apiKey")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => <span className="font-mono">{row.getValue("apiKeyName")}</span>,
			},
			{
				accessorKey: "modelId",
				meta: { title: t("requestLogs.upstreamModel") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.upstreamModel")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => <span className="font-mono">{row.getValue("modelId")}</span>,
			},
			{
				accessorKey: "success",
				meta: { title: t("requestLogs.status") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.status")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => {
					const ok = row.original.success;
					return (
						<Badge variant={ok ? "default" : "destructive"}>
							{ok ? t("requestLogs.success") : t("requestLogs.failed")}
						</Badge>
					);
				},
			},
			{
				accessorKey: "inputTokens",
				meta: { title: t("requestLogs.inputTokens") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.input")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => fmtTokens(row.original.inputTokens),
			},
			{
				accessorKey: "outputTokens",
				meta: { title: t("requestLogs.outputTokens") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.output")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => fmtTokens(row.original.outputTokens),
			},
			{
				accessorKey: "requestTime",
				meta: { title: t("requestLogs.totalTime") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.latency")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => fmtMs(row.original.requestTime),
			},
			{
				accessorKey: "startTime",
				meta: { title: t("requestLogs.time") },
				header: ({ column }) => (
					<DataTableColumnHeader
						column={column}
						title={t("requestLogs.time")}
						className={PLAIN_HEADER_CLASS}
					/>
				),
				cell: ({ row }) => fmtTime(row.original.startTime),
			},
		],
		[t],
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
			<Card className="p-3">
				<div className="flex flex-wrap items-end gap-3">
					<div className="space-y-1">
						<p className="text-xs text-muted-foreground">{t("requestLogs.virtualModel")}</p>
						<Select
							value={vmId !== undefined ? String(vmId) : "all"}
							onValueChange={(v) => {
								setVmId(v === "all" ? undefined : Number(v));
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[160px]" aria-label={t("requestLogs.filterByVm")}>
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">{t("common.all")}</SelectItem>
								{virtualModels?.map((vm) => (
									<SelectItem key={vm.virtualModelId} value={String(vm.virtualModelId)}>
										{vm.displayId}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</div>
					<div className="space-y-1">
						<p className="text-xs text-muted-foreground">{t("requestLogs.provider")}</p>
						<Select
							value={providerId !== undefined ? String(providerId) : "all"}
							onValueChange={(v) => {
								setProviderId(v === "all" ? undefined : Number(v));
								// 供应商变化后旧模型过滤不再适用，清空并回到全部。
								setModelId(undefined);
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[160px]" aria-label={t("requestLogs.filterByProvider")}>
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">{t("common.all")}</SelectItem>
								{providers?.map((p) => (
									<SelectItem key={p.id} value={String(p.id)}>
										{p.name}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</div>
					<div className="space-y-1">
						<p className="text-xs text-muted-foreground">{t("requestLogs.upstreamModel")}</p>
						<Select
							value={modelId ?? "all"}
							onValueChange={(v) => {
								setModelId(v === "all" ? undefined : v);
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[180px]" aria-label={t("requestLogs.filterByModel")}>
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">{t("common.all")}</SelectItem>
								{providerModelOptions.map((model) => (
									<SelectItem key={model.modelId} value={model.providerModelId}>
										{model.providerModelId}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</div>
					<div className="space-y-1">
						<p className="text-xs text-muted-foreground">{t("requestLogs.status")}</p>
						<Select
							value={success === undefined ? "all" : success ? "true" : "false"}
							onValueChange={(v) => {
								setSuccess(v === "all" ? undefined : v === "true");
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[100px]" aria-label={t("requestLogs.filterByStatus")}>
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">{t("common.all")}</SelectItem>
								<SelectItem value="true">{t("requestLogs.success")}</SelectItem>
								<SelectItem value="false">{t("requestLogs.failed")}</SelectItem>
							</SelectContent>
						</Select>
					</div>
					<div className="space-y-1">
						<p className="text-xs text-muted-foreground">{t("requestLogs.apiKey")}</p>
						<Select
							value={apiKey ?? "all"}
							onValueChange={(v) => {
								setApiKey(v === "all" ? undefined : v);
								setPage(1);
							}}
						>
							<SelectTrigger className="w-[160px]" aria-label={t("requestLogs.filterByApiKey")}>
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="all">{t("common.all")}</SelectItem>
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
						{t("requestLogs.reset")}
					</Button>
				</div>
				<div className="mt-3 flex flex-wrap items-center gap-3 border-t border-foreground/5 pt-3">
					<span className="text-xs text-muted-foreground">{t("requestLogs.time")}</span>
					<RaceWindowControl
						state={timeWindow}
						now={now}
						onChange={(patch) => setTimeWindow((prev) => ({ ...prev, ...patch }))}
					/>
					<div className="ml-auto">
						<DataTableViewOptions table={table} />
					</div>
				</div>
			</Card>

			{isError ? (
				<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-destructive">
					{t("common.loadFailed")}
					<Button variant="outline" size="sm" className="ml-3" onClick={() => refetch()}>
						{t("common.retry")}
					</Button>
				</div>
			) : isLoading ? (
				<TableSkeleton columns={columns.length} />
			) : (
				<Card className="overflow-x-auto">
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
							{data && data.items.length > 0 ? (
								table.getRowModel().rows.map((row) => (
									<TableRow
										key={row.id}
										className="cursor-pointer"
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
											title={t("requestLogs.noLogs")}
											description={t("requestLogs.noLogsHint")}
											className="border-0 bg-transparent shadow-none"
										/>
									</TableCell>
								</TableRow>
							)}
						</TableBody>
					</Table>
				</Card>
			)}

			{/* 服务端分页 */}
			<div className="flex flex-wrap items-center justify-between gap-4">
				<p className="text-sm text-muted-foreground">
					{t("requestLogs.totalSummary", {
						total: data?.total ?? 0,
						page,
						totalPages,
					})}
				</p>
				<div className="flex items-center gap-2">
					<Select
						value={String(pageSize)}
						onValueChange={(v) => {
							setPageSize(Number(v));
							setPage(1);
						}}
					>
						<SelectTrigger className="h-8 w-[110px]" aria-label={t("requestLogs.pageSize")}>
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							{PAGE_SIZE_OPTIONS.map((size) => (
								<SelectItem key={size} value={String(size)}>
									{size} {t("requestLogs.pageSizeSuffix")}
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
						{t("requestLogs.previousPage")}
					</Button>
					<Button
						variant="outline"
						size="sm"
						disabled={page >= totalPages}
						onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
					>
						{t("requestLogs.nextPage")}
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
