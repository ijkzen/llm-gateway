import { ProtocolIcon } from "@/components/providers/ProtocolIcon";
import { Card, CardContent } from "@/components/ui/card";
import type { Provider } from "@/hooks/use-providers";
import { cn } from "@/lib/utils";

interface ProviderListProps {
	providers: Provider[] | undefined;
	selectedId: number | null;
	onSelect: (provider: Provider) => void;
}

export function ProviderList({ providers, selectedId, onSelect }: ProviderListProps) {
	if (!providers || providers.length === 0) {
		return (
			<Card>
				<CardContent className="p-8 text-center text-muted-foreground">
					暂无 Provider，点击右上角创建
				</CardContent>
			</Card>
		);
	}

	return (
		<Card className="p-0">
			<CardContent className="p-0">
				<ul className="divide-y">
					{providers.map((provider) => (
						<li key={provider.id}>
							<button
								type="button"
								onClick={() => onSelect(provider)}
								title={`${provider.name}（${provider.enable ? "已启用" : "已停用"}）`}
								className={cn(
									"flex w-full items-center gap-3 px-4 py-3 text-left transition-colors",
									selectedId === provider.id
										? "bg-foreground text-background dark:bg-primary dark:text-primary-foreground"
										: "hover:bg-slate-100/60 dark:hover:bg-white/5",
								)}
							>
								<ProtocolIcon
									protocolType={provider.protocolType}
									className={
										selectedId === provider.id
											? "text-background dark:text-primary-foreground"
											: "text-muted-foreground"
									}
								/>
								<span className="min-w-0 flex-1 truncate font-medium">{provider.name}</span>
								<span
									className={cn(
										"size-2 shrink-0 rounded-full",
										provider.enable ? "bg-emerald-500" : "bg-red-500",
									)}
									aria-label={provider.enable ? "已启用" : "已停用"}
								/>
							</button>
						</li>
					))}
				</ul>
			</CardContent>
		</Card>
	);
}
