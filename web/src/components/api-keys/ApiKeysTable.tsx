import { ApiKeyCell } from "@/components/api-keys/ApiKeyCell";
import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTablePagination } from "@/components/data-table/pagination";
import { EmptyState } from "@/components/empty-state";
import { RelativeTime } from "@/components/relative-time";
import { StatusBadge } from "@/components/status-badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import type { ApiKey } from "@/hooks/use-api-keys";
import { useToggleApiKey } from "@/hooks/use-api-keys";
import { useToastActions } from "@/hooks/use-toast";
import {
	type ColumnDef,
	type PaginationState,
	type SortingState,
	flexRender,
	getCoreRowModel,
	getPaginationRowModel,
	getSortedRowModel,
	useReactTable,
} from "@tanstack/react-table";
import { ChevronRight, MoreHorizontal, Power, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

interface ApiKeysTableProps {
	apiKeys: ApiKey[] | undefined;
	onDelete: (apiKey: ApiKey) => void;
}

const PLAIN_HEADER_CLASS = "text-xs font-medium uppercase tracking-wider text-muted-foreground";

export function ApiKeysTable({ apiKeys, onDelete }: ApiKeysTableProps) {
	const { t } = useTranslation();
	const navigate = useNavigate();
	const { toastSuccess, toastError } = useToastActions();
	const toggleApiKey = useToggleApiKey();
	const [sorting, setSorting] = useState<SortingState>([]);
	const [pagination, setPagination] = useState<PaginationState>({ pageIndex: 0, pageSize: 10 });

	const toggleEnable = (apiKey: ApiKey) => {
		toggleApiKey.mutate(
			{ id: apiKey.id, enable: !apiKey.enable },
			{
				onSuccess: () => toastSuccess(t("common.success")),
				onError: (error) => toastError(t("common.error"), error),
			},
		);
	};

	// 列定义依赖 toggleEnable/onDelete 等每次渲染重建的闭包，不做 useMemo
	// （小表格重算列定义开销可忽略，且 biome 禁止不稳定函数作为依赖）。
	const columns: ColumnDef<ApiKey>[] = [
		{
			accessorKey: "name",
			meta: { title: t("apiKeys.name") },
			header: ({ column }) => (
				<DataTableColumnHeader
					column={column}
					title={t("apiKeys.name")}
					className={PLAIN_HEADER_CLASS}
				/>
			),
			cell: ({ row }) => {
				const apiKey = row.original;
				return (
					<button
						type="button"
						data-nav
						onClick={() => navigate(`/api-keys/${apiKey.id}/overview`)}
						title={t("apiKeys.viewOverview", { key: apiKey.name })}
						className="group flex max-w-full cursor-pointer items-center gap-0.5 rounded-md px-1 py-0.5 text-left font-medium transition-colors hover:bg-muted/60"
					>
						<span className="truncate">{apiKey.name}</span>
						<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
					</button>
				);
			},
		},
		{
			accessorKey: "keyMasked",
			meta: { title: t("apiKeys.keyColumn") },
			enableSorting: false,
			header: () => <div className={PLAIN_HEADER_CLASS}>{t("apiKeys.keyColumn")}</div>,
			cell: ({ row }) => <ApiKeyCell apiKey={row.original} />,
		},
		{
			accessorKey: "enable",
			meta: { title: t("apiKeys.enable") },
			header: () => <div className={PLAIN_HEADER_CLASS}>{t("apiKeys.enable")}</div>,
			cell: ({ row }) => {
				const apiKey = row.original;
				return (
					<div className="flex items-center gap-2">
						<Switch
							checked={apiKey.enable}
							disabled={toggleApiKey.isPending}
							aria-label={`${t("apiKeys.toggleStatus")} ${apiKey.name} ${t("apiKeys.toggleStatusSuffix")}`}
							onCheckedChange={() => toggleEnable(apiKey)}
						/>
						<StatusBadge status={apiKey.enable ? "enabled" : "disabled"} />
					</div>
				);
			},
		},
		{
			accessorKey: "createdAt",
			meta: { title: t("apiKeys.createdAt") },
			header: ({ column }) => (
				<DataTableColumnHeader
					column={column}
					title={t("apiKeys.createdAt")}
					className={PLAIN_HEADER_CLASS}
				/>
			),
			cell: ({ row }) => <RelativeTime date={row.getValue("createdAt")} />,
		},
		{
			id: "actions",
			enableHiding: false,
			header: () => (
				<div className={`text-right ${PLAIN_HEADER_CLASS}`}>{t("apiKeys.operate")}</div>
			),
			cell: ({ row }) => {
				const apiKey = row.original;
				return (
					<div className="text-right">
						<DropdownMenu modal={false}>
							<DropdownMenuTrigger asChild>
								<Button
									variant="ghost"
									size="icon"
									className="size-8"
									aria-label={`${t("apiKeys.operate")} ${apiKey.name}`}
								>
									<MoreHorizontal className="size-4" />
								</Button>
							</DropdownMenuTrigger>
							<DropdownMenuContent align="end">
								<DropdownMenuItem onClick={() => toggleEnable(apiKey)}>
									<Power className="size-4" />
									{apiKey.enable ? t("common.disable") : t("common.enable")}
								</DropdownMenuItem>
								<DropdownMenuSeparator />
								<DropdownMenuItem variant="destructive" onClick={() => onDelete(apiKey)}>
									<Trash2 className="size-4" />
									{t("apiKeys.delete")}
								</DropdownMenuItem>
							</DropdownMenuContent>
						</DropdownMenu>
					</div>
				);
			},
		},
	];

	const table = useReactTable({
		data: apiKeys ?? [],
		columns,
		state: { sorting, pagination },
		onSortingChange: setSorting,
		onPaginationChange: setPagination,
		getCoreRowModel: getCoreRowModel(),
		getSortedRowModel: getSortedRowModel(),
		getPaginationRowModel: getPaginationRowModel(),
	});

	// 数据变化时回到第一页，避免停留在空页。
	// biome-ignore lint/correctness/useExhaustiveDependencies: apiKeys 变化本身就是重置页码的触发条件
	useEffect(() => {
		setPagination((prev) => ({ ...prev, pageIndex: 0 }));
	}, [apiKeys]);

	const rows = table.getRowModel().rows;

	return (
		<div className="space-y-4">
			{rows.length === 0 ? (
				<EmptyState
					title={t("apiKeys.noKeys")}
					description={t("apiKeys.noKeysHint")}
					className="border-0 bg-transparent shadow-none"
				/>
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
							{rows.map((row) => (
								<TableRow key={row.id}>
									{row.getVisibleCells().map((cell) => (
										<TableCell key={cell.id}>
											{flexRender(cell.column.columnDef.cell, cell.getContext())}
										</TableCell>
									))}
								</TableRow>
							))}
						</TableBody>
					</Table>
				</Card>
			)}
			<DataTablePagination table={table} />
		</div>
	);
}
