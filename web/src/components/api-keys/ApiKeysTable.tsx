import { ApiKeyCell } from "@/components/api-keys/ApiKeyCell";
import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTablePagination } from "@/components/data-table/pagination";
import { EmptyState } from "@/components/empty-state";
import { RelativeTime } from "@/components/relative-time";
import { StatusBadge } from "@/components/status-badge";
import { Button } from "@/components/ui/button";
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
import { MoreHorizontal, Power, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

interface ApiKeysTableProps {
	apiKeys: ApiKey[] | undefined;
	onDelete: (apiKey: ApiKey) => void;
}

const PLAIN_HEADER_CLASS = "text-xs font-medium uppercase tracking-wider text-muted-foreground";

export function ApiKeysTable({ apiKeys, onDelete }: ApiKeysTableProps) {
	const { toastSuccess, toastError } = useToastActions();
	const toggleApiKey = useToggleApiKey();
	const [sorting, setSorting] = useState<SortingState>([]);
	const [pagination, setPagination] = useState<PaginationState>({ pageIndex: 0, pageSize: 10 });

	const toggleEnable = (apiKey: ApiKey) => {
		toggleApiKey.mutate(
			{ id: apiKey.id, enable: !apiKey.enable },
			{
				onSuccess: () => toastSuccess("操作成功"),
				onError: (error) => toastError("操作失败", error),
			},
		);
	};

	// 列定义依赖 toggleEnable/onDelete 等每次渲染重建的闭包，不做 useMemo
	// （小表格重算列定义开销可忽略，且 biome 禁止不稳定函数作为依赖）。
	const columns: ColumnDef<ApiKey>[] = [
		{
			accessorKey: "name",
			meta: { title: "名称" },
			header: ({ column }) => (
				<DataTableColumnHeader column={column} title="名称" className={PLAIN_HEADER_CLASS} />
			),
			cell: ({ row }) => <span className="font-medium">{row.getValue("name")}</span>,
		},
		{
			accessorKey: "keyMasked",
			meta: { title: "Key" },
			enableSorting: false,
			header: () => <div className={PLAIN_HEADER_CLASS}>Key</div>,
			cell: ({ row }) => <ApiKeyCell apiKey={row.original} />,
		},
		{
			accessorKey: "enable",
			meta: { title: "状态" },
			header: () => <div className={PLAIN_HEADER_CLASS}>状态</div>,
			cell: ({ row }) => {
				const apiKey = row.original;
				return (
					<div className="flex items-center gap-2">
						<Switch
							checked={apiKey.enable}
							disabled={toggleApiKey.isPending}
							aria-label={`切换 API Key ${apiKey.name} 状态`}
							onCheckedChange={() => toggleEnable(apiKey)}
						/>
						<StatusBadge status={apiKey.enable ? "enabled" : "disabled"} />
					</div>
				);
			},
		},
		{
			accessorKey: "createdAt",
			meta: { title: "创建时间" },
			header: ({ column }) => (
				<DataTableColumnHeader column={column} title="创建时间" className={PLAIN_HEADER_CLASS} />
			),
			cell: ({ row }) => <RelativeTime date={row.getValue("createdAt")} />,
		},
		{
			id: "actions",
			enableHiding: false,
			header: () => <div className={`text-right ${PLAIN_HEADER_CLASS}`}>操作</div>,
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
									aria-label={`操作 ${apiKey.name}`}
								>
									<MoreHorizontal className="size-4" />
								</Button>
							</DropdownMenuTrigger>
							<DropdownMenuContent align="end">
								<DropdownMenuItem onClick={() => toggleEnable(apiKey)}>
									<Power className="size-4" />
									{apiKey.enable ? "禁用" : "启用"}
								</DropdownMenuItem>
								<DropdownMenuSeparator />
								<DropdownMenuItem variant="destructive" onClick={() => onDelete(apiKey)}>
									<Trash2 className="size-4" />
									删除
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
						{rows.length > 0 ? (
							rows.map((row) => (
								<TableRow key={row.id} className="transition-colors hover:bg-muted/50">
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
										title="暂无 API Key"
										description="创建一个 API Key 供调用方访问网关"
										className="border-0 bg-transparent shadow-none"
									/>
								</TableCell>
							</TableRow>
						)}
					</TableBody>
				</Table>
			</div>
			<DataTablePagination table={table} />
		</div>
	);
}
