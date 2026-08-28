import { Skeleton } from "@/components/ui/skeleton";

export function PageHeaderSkeleton() {
	return (
		<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
			<div className="flex items-start gap-3">
				<Skeleton className="size-10 rounded-xl" />
				<div className="space-y-2">
					<Skeleton className="h-9 w-48" />
					<Skeleton className="h-5 w-64" />
				</div>
			</div>
		</div>
	);
}
