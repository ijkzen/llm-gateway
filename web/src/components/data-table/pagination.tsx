import { Button } from "@/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { cn, getPageNumbers } from "@/lib/utils";
import type { Table } from "@tanstack/react-table";
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from "lucide-react";

interface DataTablePaginationProps<TData> {
	table: Table<TData>;
	className?: string;
}

export function DataTablePagination<TData>({ table, className }: DataTablePaginationProps<TData>) {
	const currentPage = table.getState().pagination.pageIndex + 1;
	const totalPages = Math.max(table.getPageCount(), 1);
	const pageNumbers = getPageNumbers(currentPage, totalPages);

	return (
		<div
			className={cn(
				"flex flex-col-reverse items-center justify-between gap-4 px-2 sm:flex-row",
				className,
			)}
		>
			<div className="flex items-center gap-2">
				<p className="hidden text-sm font-medium sm:block">每页条数</p>
				<Select
					value={`${table.getState().pagination.pageSize}`}
					onValueChange={(value) => {
						table.setPageSize(Number(value));
					}}
				>
					<SelectTrigger className="h-8 w-[70px]" aria-label="每页条数">
						<SelectValue placeholder={table.getState().pagination.pageSize} />
					</SelectTrigger>
					<SelectContent side="top">
						{[10, 20, 30, 40, 50].map((pageSize) => (
							<SelectItem key={pageSize} value={`${pageSize}`}>
								{pageSize}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			</div>

			<div className="flex items-center gap-2 sm:gap-6">
				<div className="flex w-[100px] items-center justify-center text-sm font-medium">
					第 {currentPage} / {totalPages} 页
				</div>
				<div className="flex items-center space-x-2">
					<Button
						variant="outline"
						className="hidden size-8 p-0 lg:flex"
						onClick={() => table.setPageIndex(0)}
						disabled={!table.getCanPreviousPage()}
					>
						<span className="sr-only">跳转到首页</span>
						<ChevronsLeft className="size-4" />
					</Button>
					<Button
						variant="outline"
						className="size-8 p-0"
						onClick={() => table.previousPage()}
						disabled={!table.getCanPreviousPage()}
						aria-label="上一页"
					>
						<ChevronLeft className="size-4" />
					</Button>

					{pageNumbers.map((pageNumber, index) =>
						pageNumber === "..." ? (
							<span
								key={`ellipsis-${pageNumbers[index + 1]}`}
								className="px-1 text-sm text-muted-foreground"
							>
								...
							</span>
						) : (
							<Button
								key={pageNumber}
								variant={currentPage === pageNumber ? "default" : "outline"}
								className="h-8 min-w-8 px-2"
								onClick={() => table.setPageIndex(pageNumber - 1)}
								aria-label={`第 ${pageNumber} 页`}
							>
								{pageNumber}
							</Button>
						),
					)}

					<Button
						variant="outline"
						className="size-8 p-0"
						onClick={() => table.nextPage()}
						disabled={!table.getCanNextPage()}
						aria-label="下一页"
					>
						<ChevronRight className="size-4" />
					</Button>
					<Button
						variant="outline"
						className="hidden size-8 p-0 lg:flex"
						onClick={() => table.setPageIndex(table.getPageCount() - 1)}
						disabled={!table.getCanNextPage()}
					>
						<span className="sr-only">跳转到末页</span>
						<ChevronsRight className="size-4" />
					</Button>
				</div>
			</div>
		</div>
	);
}
