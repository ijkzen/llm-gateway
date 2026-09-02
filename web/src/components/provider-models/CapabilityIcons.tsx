import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import type { ProviderModel } from "@/hooks/use-provider-models";
import { cn } from "@/lib/utils";
import { Brain, Image, type LucideIcon, Video, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";

/** 模型能力定义：key 对应 ProviderModel 上的布尔字段。 */
export const CAPABILITIES: {
	key: keyof Pick<ProviderModel, "reasoning" | "toolUse" | "imageUnderstand" | "videoUnderstand">;
	labelKey: string;
	icon: LucideIcon;
}[] = [
	{ key: "reasoning", labelKey: "providerModels.reasoning", icon: Brain },
	{ key: "toolUse", labelKey: "providerModels.toolUse", icon: Wrench },
	{ key: "imageUnderstand", labelKey: "providerModels.imageUnderstand", icon: Image },
	{ key: "videoUnderstand", labelKey: "providerModels.videoUnderstand", icon: Video },
];

/** 以图标形式展示模型已具备的能力（仅展示为 true 的项，tooltip 说明含义）。 */
export function CapabilityIcons({
	model,
	className,
}: {
	model: ProviderModel;
	className?: string;
}) {
	const { t } = useTranslation();
	return (
		<TooltipProvider delayDuration={200}>
			<div className={cn("flex items-center gap-1.5", className)}>
				{CAPABILITIES.map(({ key, labelKey, icon: Icon }) => {
					const label = t(labelKey);
					return model[key] ? (
						<Tooltip key={key}>
							<TooltipTrigger asChild>
								<span
									aria-label={label}
									className="flex size-6 items-center justify-center rounded-md bg-success/10 text-success"
								>
									<Icon className="size-3.5" />
								</span>
							</TooltipTrigger>
							<TooltipContent>{label}</TooltipContent>
						</Tooltip>
					) : null;
				})}
			</div>
		</TooltipProvider>
	);
}
