import { cn } from "@/lib/utils";
import type { LucideIcon } from "lucide-react";

interface EmptyStateProps {
	icon?: LucideIcon;
	title: string;
	description?: string;
	action?: React.ReactNode;
	className?: string;
}

export function EmptyState({ icon: Icon, title, description, action, className }: EmptyStateProps) {
	return (
		<div
			className={cn(
				"flex flex-col items-center justify-center rounded-xl border bg-card p-8 text-center shadow-sm",
				className,
			)}
		>
			{Icon && <Icon className="mb-4 size-12 text-muted-foreground/50" />}
			<h3 className="text-lg font-semibold">{title}</h3>
			{description && <p className="mt-1 text-sm text-muted-foreground">{description}</p>}
			{action && <div className="mt-4">{action}</div>}
		</div>
	);
}
