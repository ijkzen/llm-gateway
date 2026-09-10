import { DataTableColumnHeader } from "@/components/data-table/column-header";
import { DataTablePagination } from "@/components/data-table/pagination";
import { DataTableViewOptions } from "@/components/data-table/view-options";
import { EmptyState } from "@/components/empty-state";
import { MidEllipsis } from "@/components/mid-ellipsis";
import { RelativeTime } from "@/components/relative-time";
import { SearchInput } from "@/components/search-input";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Setting } from "@/hooks/use-settings";
import { SETTING_TYPES, type SettingType } from "@/lib/constants";
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
import { useTranslation } from "react-i18next";

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

/** 后端拒绝删除的内置设置键（18-20：前端同步置灰删除入口，不再靠 400 兜底）。 */
const PROTECTED_KEYS = new Set(["language", "timezone"]);

export function SettingsTable({ settings, onEdit, onDelete }: SettingsTableProps) {
	const { t } = useTranslation();
	const [searchQuery, setSearchQuery] = useState("");
	const [typeFilter, setTypeFilter] = useState("all");
	const [sorting, setSorting] = useState<SortingState>([]);
	const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
	const [pagination, setPagination] = useState<PaginationState>({ pageIndex: 0, pageSize: 10 });

	const filteredSettings = useMemo(() => {
		let list = settings ?? [];
		if (searchQuery.trim()) {
			const q = searchQuery.toLowerCase();
			list = list.filter(
				(s) => s.key.toLowerCase().includes(q) || s.value.toLowerCase().includes(q),
			);
		}
		if (typeFilter !== "all") {
			list = list.filter((s) => s.type === typeFilter);
		}
		return list;
	}, [settings, searchQuery, typeFilter]);

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
										disabled={PROTECTED_KEYS.has(setting.key)}
										title={
											PROTECTED_KEYS.has(setting.key)
												? t("settings.builtinNotDeletable")
												: undefined
										}
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
		[onEdit, onDelete, t],
	);

	const table = useReactTable({
		data: filteredSettings,
		columns,
		state: { sorting, columnVisibility, pagination },
		onSortingChange: setSorting,
		onColumnVisibilityChange: setColumnVisibility,
		onPaginationChange: setPagination,
		getCoreRowModel: getCoreRowModel(),
		getSortedRowModel: getSortedRowModel(),
		getPaginationRowModel: getPaginationRowModel(),
	});

	// 搜索/筛选导致结果变化时回到第一页，避免停留在越界空页。
	// 18-18：此前依赖 [settings]，搜索收窄结果时不触发（实测 TanStack 的
	// autoResetPageIndex 在此场景不生效）——改依赖已过滤结果。
	// biome-ignore lint/correctness/useExhaustiveDependencies: 过滤结果变化即重置页码，setPagination 为稳定 setter
	useEffect(() => {
		setPagination((prev) => ({ ...prev, pageIndex: 0 }));
	}, [filteredSettings]);

	const rows = table.getRowModel().rows;

	return (
		<div className="space-y-4">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<SearchInput
					value={searchQuery}
					onChange={setSearchQuery}
					placeholder={t("settings.searchPlaceholder")}
				/>
				<div className="flex items-center gap-2 sm:justify-end">
					<Select value={typeFilter} onValueChange={setTypeFilter}>
						<SelectTrigger className="w-[160px]" aria-label={t("settings.filterByType")}>
							<SelectValue placeholder={t("settings.allTypes")} />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="all">{t("settings.allTypes")}</SelectItem>
							{SETTING_TYPES.map((type) => (
								<SelectItem key={type} value={type}>
									{type}
								</SelectItem>
							))}
							{/* 18-19：后端对无法映射的存储类型返回 Unknown，补筛选项（徽章已有兜底）。 */}
							<SelectItem value="Unknown">Unknown</SelectItem>
						</SelectContent>
					</Select>
					<DataTableViewOptions table={table} />
				</div>
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
