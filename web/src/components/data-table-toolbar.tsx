import type { ReactNode } from "react";

interface DataTableToolbarProps {
	children: ReactNode;
}

export function DataTableToolbar({ children }: DataTableToolbarProps) {
	return (
		<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
			{children}
		</div>
	);
}
