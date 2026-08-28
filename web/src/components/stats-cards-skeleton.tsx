import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";

export function StatsCardsSkeleton({ count = 4 }: { count?: number }) {
	return (
		<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
			{Array.from({ length: count }).map((_, i) => (
				<Card
					/* biome-ignore lint/suspicious/noArrayIndexKey: static skeleton placeholders */
					key={i}
				>
					<CardContent className="flex items-start gap-4 p-6">
						<Skeleton className="size-10 rounded-xl" />
						<div className="space-y-2">
							<Skeleton className="h-4 w-20" />
							<Skeleton className="h-8 w-16" />
						</div>
					</CardContent>
				</Card>
			))}
		</div>
	);
}
