import type { LucideIcon } from "lucide-react";

interface PageHeaderProps {
	icon?: LucideIcon;
	title: string;
	children?: React.ReactNode;
}

export function PageHeader({ icon: Icon, title, children }: PageHeaderProps) {
	return (
		<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
			<div className="flex items-start gap-3">
				{Icon && (
					<div className="flex size-11 shrink-0 items-center justify-center rounded-2xl bg-foreground/5 text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.6)] dark:bg-white/5">
						<Icon className="size-5" />
					</div>
				)}
				<h1 className="text-3xl font-bold tracking-tight">{title}</h1>
			</div>
			{children && <div className="flex items-center gap-2">{children}</div>}
		</div>
	);
}
