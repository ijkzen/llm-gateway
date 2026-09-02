import { cn } from "@/lib/utils";

function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
	return (
		<div
			className={cn("animate-pulse rounded-lg bg-muted/60 dark:bg-white/[0.07]", className)}
			{...props}
		/>
	);
}

export { Skeleton };
