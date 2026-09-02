import { Skeleton } from "@/components/ui/skeleton";

export function PageHeaderSkeleton() {
	return (
		<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
			<div className="flex items-start gap-3">
				<Skeleton className="size-11 rounded-2xl" />
				<Skeleton className="h-9 w-48" />
			</div>
		</div>
	);
}
