import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTablePagination } from "@/components/data-table/pagination";
import { DataTableViewOptions } from "@/components/data-table/view-options";
import { EmptyState } from "@/components/empty-state";
import { MidEllipsis } from "@/components/mid-ellipsis";
import { RelativeTime } from "@/components/relative-time";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
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
import { MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

interface SettingsTableProps {
	settings: Setting[] | undefined;
	onEdit: (setting: Setting) => void;
	onDelete: (setting: Setting) => void;
}

const TYPE_BADGE_VARIANTS: Record<SettingType, string> = {
	String: "bg-info/10 text-info hover:bg-info/10",
	Int: "bg-info/10 text-info hover:bg-info/10",
	Float: "bg-warning/10 text-warning hover:bg-warning/10",
	Bool: "bg-success/10 text-success hover:bg-success/10",
	Json: "bg-primary/10 text-primary hover:bg-primary/10",
};

const FALLBACK_TYPE_BADGE_VARIANT = "bg-muted text-muted-foreground hover:bg-muted";

function getTypeBadgeVariant(type: SettingType) {
	// 后端对无法映射的存储类型会返回 "Unknown"（见 SettingResponse），运行时兜底不可省
	return TYPE_BADGE_VARIANTS[type] ?? FALLBACK_TYPE_BADGE_VARIANT;
}

const PLAIN_HEADER_CLASS = "text-xs font-medium uppercase tracking-wider text-muted-foreground";

export function SettingsTable({ settings, onEdit, onDelete }: SettingsTableProps) {
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
								<span className="block max-w-xs">
									<MidEllipsis text={value} />
								</span>
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
									<DropdownMenuItem
										className="text-destructive focus:text-destructive"
										onClick={() => onDelete(setting)}
									>
										<Trash2 className="size-4" />
										删除
									</DropdownMenuItem>
								</DropdownMenuContent>
							</DropdownMenu>
						</div>
					);
				},
			},
		],
		[onEdit, onDelete],
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
			{rows.length === 0 ? (
				<EmptyState
					title="暂无设置项"
					description="没有找到任何系统配置项"
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
