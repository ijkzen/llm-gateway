import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTablePagination } from "@/components/data-table/pagination";
import { DataTableViewOptions } from "@/components/data-table/view-options";
import { EmptyState } from "@/components/empty-state";
import { RelativeTime } from "@/components/relative-time";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Setting } from "@/hooks/use-settings";
import type { SettingType } from "@/lib/constants";
import { cn } from "@/lib/utils";
import {
	type ColumnDef,
	type PaginationState,
	type SortingState,
	type VisibilityState,
	flexRender,
	getCoreRowModel,
	getPaginationRowModel,
	getSortedRowModel,
	useReactTable,
} from "@tanstack/react-table";
import { MoreHorizontal, Pencil } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

interface SettingsTableProps {
	settings: Setting[] | undefined;
	onEdit: (setting: Setting) => void;
}

const TYPE_BADGE_VARIANTS: Record<SettingType, string> = {
	String:
		"bg-indigo-50 text-indigo-600 hover:bg-indigo-50 dark:bg-indigo-500/15 dark:text-indigo-400",
	Int: "bg-blue-50 text-blue-600 hover:bg-blue-50 dark:bg-blue-500/15 dark:text-blue-400",
	Float: "bg-amber-50 text-amber-600 hover:bg-amber-50 dark:bg-amber-500/15 dark:text-amber-400",
	Bool: "bg-emerald-50 text-emerald-600 hover:bg-emerald-50 dark:bg-emerald-500/15 dark:text-emerald-400",
};

const FALLBACK_TYPE_BADGE_VARIANT =
	"bg-slate-100 text-slate-600 hover:bg-slate-100 dark:bg-white/5 dark:text-slate-400";

function getTypeBadgeVariant(type: SettingType) {
	// 后端对无法映射的存储类型会返回 "Unknown"（见 SettingResponse），运行时兜底不可省
	return TYPE_BADGE_VARIANTS[type] ?? FALLBACK_TYPE_BADGE_VARIANT;
}

const PLAIN_HEADER_CLASS = "text-xs font-medium uppercase tracking-wider text-muted-foreground";

export function SettingsTable({ settings, onEdit }: SettingsTableProps) {
	const [sorting, setSorting] = useState<SortingState>([]);
	const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
	const [pagination, setPagination] = useState<PaginationState>({ pageIndex: 0, pageSize: 10 });

	const columns = useMemo<ColumnDef<Setting>[]>(
		() => [
			{
				accessorKey: "key",
				meta: { title: "键" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="键" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => <span className="font-medium">{row.getValue("key")}</span>,
			},
			{
				accessorKey: "value",
				meta: { title: "值" },
				enableSorting: false,
				header: () => <div className={PLAIN_HEADER_CLASS}>值</div>,
				cell: ({ row }) => {
					const value: string = row.getValue("value");
					return (
						<Tooltip>
							<TooltipTrigger asChild>
								<span className="block max-w-xs truncate">{value}</span>
							</TooltipTrigger>
							<TooltipContent>
								<p className="max-w-md break-all">{value}</p>
							</TooltipContent>
						</Tooltip>
					);
				},
			},
			{
				accessorKey: "type",
				meta: { title: "类型" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="类型" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => {
					const type = row.getValue<SettingType>("type");
					return <Badge className={getTypeBadgeVariant(type)}>{type}</Badge>;
				},
			},
			{
				accessorKey: "updated_at",
				meta: { title: "更新时间" },
				header: ({ column }) => (
					<DataTableColumnHeader column={column} title="更新时间" className={PLAIN_HEADER_CLASS} />
				),
				cell: ({ row }) => <RelativeTime date={row.getValue("updated_at")} />,
			},
			{
				id: "actions",
				enableHiding: false,
				header: () => <div className={`text-right ${PLAIN_HEADER_CLASS}`}>操作</div>,
				cell: ({ row }) => {
					const setting = row.original;
					return (
						<div className="text-right">
							<DropdownMenu modal={false}>
								<DropdownMenuTrigger asChild>
									<Button
										variant="ghost"
										size="icon"
										className="size-8"
										aria-label={`操作 ${setting.key}`}
									>
										<MoreHorizontal className="size-4" />
									</Button>
								</DropdownMenuTrigger>
								<DropdownMenuContent align="end">
									<DropdownMenuItem onClick={() => onEdit(setting)}>
										<Pencil className="size-4" />
										编辑
									</DropdownMenuItem>
								</DropdownMenuContent>
							</DropdownMenu>
						</div>
					);
				},
			},
		],
		[onEdit],
	);

	const table = useReactTable({
		data: settings ?? [],
		columns,
		state: { sorting, columnVisibility, pagination },
		onSortingChange: setSorting,
		onColumnVisibilityChange: setColumnVisibility,
		onPaginationChange: setPagination,
		getCoreRowModel: getCoreRowModel(),
		getSortedRowModel: getSortedRowModel(),
		getPaginationRowModel: getPaginationRowModel(),
	});

	// 搜索/筛选导致数据变化时回到第一页，避免停留在空页
	// biome-ignore lint/correctness/useExhaustiveDependencies: settings 变化本身就是重置页码的触发条件
	useEffect(() => {
		setPagination((prev) => ({ ...prev, pageIndex: 0 }));
	}, [settings]);

	const rows = table.getRowModel().rows;

	return (
		<div className="space-y-4">
			<div className="flex justify-end">
				<DataTableViewOptions table={table} />
			</div>
			<div
				className={cn(
					"overflow-x-auto rounded-2xl border border-white/70 bg-white/65 shadow-[0_4px_16px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04] dark:shadow-[0_10px_24px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.06)]",
					rows.length === 0 && "border-0 bg-transparent shadow-none dark:bg-transparent",
				)}
			>
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
										title="暂无设置项"
										description="没有找到任何系统配置项"
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
