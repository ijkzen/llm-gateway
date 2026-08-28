import { Skeleton } from "@/components/ui/skeleton";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";

interface TableSkeletonProps {
	columns: number;
	rows?: number;
}

export function TableSkeleton({ columns, rows = 5 }: TableSkeletonProps) {
	return (
		<div className="rounded-xl border bg-card shadow-sm">
			<Table>
				<TableHeader>
					<TableRow>
						{Array.from({ length: columns }).map((_, i) => (
							// biome-ignore lint/suspicious/noArrayIndexKey: skeleton placeholders have no stable id
							<TableHead key={i}>
								<Skeleton className="h-4 w-20" />
							</TableHead>
						))}
					</TableRow>
				</TableHeader>
				<TableBody>
					{Array.from({ length: rows }).map((_, rowIndex) => (
						// biome-ignore lint/suspicious/noArrayIndexKey: skeleton placeholders have no stable id
						<TableRow key={rowIndex}>
							{Array.from({ length: columns }).map((_, colIndex) => (
								// biome-ignore lint/suspicious/noArrayIndexKey: skeleton placeholders have no stable id
								<TableCell key={colIndex}>
									<Skeleton className="h-4 w-full max-w-[120px]" />
								</TableCell>
							))}
						</TableRow>
					))}
				</TableBody>
			</Table>
		</div>
	);
}
