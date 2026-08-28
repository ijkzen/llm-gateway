import type { LucideIcon } from "lucide-react";

interface PageHeaderProps {
	icon?: LucideIcon;
	title: string;
	description?: string;
	children?: React.ReactNode;
}

export function PageHeader({ icon: Icon, title, description, children }: PageHeaderProps) {
	return (
		<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
			<div className="flex items-start gap-3">
				{Icon && (
					<div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
						<Icon className="size-5" />
					</div>
				)}
				<div>
					<h1 className="text-3xl font-bold tracking-tight">{title}</h1>
					{description && <p className="text-base text-muted-foreground">{description}</p>}
				</div>
			</div>
			{children && <div className="flex items-center gap-2">{children}</div>}
		</div>
	);
}
