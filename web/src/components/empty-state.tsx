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
				"flex flex-col items-center justify-center rounded-2xl border border-white/70 bg-white/60 p-8 text-center shadow-[0_4px_16px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04] dark:shadow-[0_10px_24px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.06)]",
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
