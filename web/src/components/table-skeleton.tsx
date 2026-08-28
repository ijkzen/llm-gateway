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
		<div className="rounded-2xl border border-white/70 bg-white/65 shadow-[0_4px_16px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04] dark:shadow-[0_10px_24px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.06)]">
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
