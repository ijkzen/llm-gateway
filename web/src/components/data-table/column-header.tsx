import { Button } from "@/components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { Column } from "@tanstack/react-table";
import { ArrowDown, ArrowUp, ChevronsUpDown, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";

interface DataTableColumnHeaderProps<TData, TValue> extends React.HTMLAttributes<HTMLDivElement> {
	column: Column<TData, TValue>;
	title: string;
}

export function DataTableColumnHeader<TData, TValue>({
	column,
	title,
	className,
}: DataTableColumnHeaderProps<TData, TValue>) {
	const { t } = useTranslation();
	if (!column.getCanSort()) {
		return <div className={cn(className)}>{title}</div>;
	}

	return (
		<div className={cn("flex items-center space-x-2", className)}>
			<DropdownMenu modal={false}>
				<DropdownMenuTrigger asChild>
					<Button variant="ghost" size="sm" className="-ml-3 h-8 data-[state=open]:bg-accent">
						<span>{title}</span>
						{column.getIsSorted() === "desc" ? (
							<ArrowDown className="ml-2 size-4" />
						) : column.getIsSorted() === "asc" ? (
							<ArrowUp className="ml-2 size-4" />
						) : (
							<ChevronsUpDown className="ml-2 size-4" />
						)}
					</Button>
				</DropdownMenuTrigger>
				<DropdownMenuContent align="start">
					<DropdownMenuItem onClick={() => column.toggleSorting(false)}>
						<ArrowUp className="size-3.5 text-muted-foreground/70" />
						{t("dataTable.sortAsc")}
					</DropdownMenuItem>
					<DropdownMenuItem onClick={() => column.toggleSorting(true)}>
						<ArrowDown className="size-3.5 text-muted-foreground/70" />
						{t("dataTable.sortDesc")}
					</DropdownMenuItem>
					{column.getCanHide() && (
						<>
							<DropdownMenuSeparator />
							<DropdownMenuItem onClick={() => column.toggleVisibility(false)}>
								<EyeOff className="size-3.5 text-muted-foreground/70" />
								{t("dataTable.hide")}
							</DropdownMenuItem>
						</>
					)}
				</DropdownMenuContent>
			</DropdownMenu>
		</div>
	);
}
