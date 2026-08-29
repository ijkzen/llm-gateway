import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import type { ProviderModel } from "@/hooks/use-provider-models";
import { cn } from "@/lib/utils";
import { Brain, Image, type LucideIcon, Video, Wrench } from "lucide-react";

/** 模型能力定义：key 对应 ProviderModel 上的布尔字段。 */
export const CAPABILITIES: {
	key: keyof Pick<ProviderModel, "reasoning" | "toolUse" | "imageUnderstand" | "videoUnderstand">;
	label: string;
	icon: LucideIcon;
}[] = [
	{ key: "reasoning", label: "推理", icon: Brain },
	{ key: "toolUse", label: "工具调用", icon: Wrench },
	{ key: "imageUnderstand", label: "图像理解", icon: Image },
	{ key: "videoUnderstand", label: "视频理解", icon: Video },
];

/** 以图标形式展示模型已具备的能力（仅展示为 true 的项，tooltip 说明含义）。 */
export function CapabilityIcons({
	model,
	className,
}: {
	model: ProviderModel;
	className?: string;
}) {
	return (
		<TooltipProvider delayDuration={200}>
			<div className={cn("flex items-center gap-1.5", className)}>
				{CAPABILITIES.map(({ key, label, icon: Icon }) =>
					model[key] ? (
						<Tooltip key={key}>
							<TooltipTrigger asChild>
								<span
									aria-label={label}
									className="flex size-6 items-center justify-center rounded-md bg-emerald-500/10 text-emerald-600 dark:bg-emerald-400/10 dark:text-emerald-400"
								>
									<Icon className="size-3.5" />
								</span>
							</TooltipTrigger>
							<TooltipContent>{label}</TooltipContent>
						</Tooltip>
					) : null,
				)}
			</div>
		</TooltipProvider>
	);
}
