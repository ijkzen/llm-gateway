import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { Brain, Image, type LucideIcon, Video, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";

/** 能力图标的数据来源：供应商模型与虚拟模型成员条目的公共子集（17-26 合一）。 */
export type CapabilityKey = "reasoning" | "toolUse" | "imageUnderstand" | "videoUnderstand";
export type CapabilitySource = Record<CapabilityKey, boolean>;

/** 模型能力定义：key 对应能力布尔字段。 */
export const CAPABILITIES: {
	key: CapabilityKey;
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
	/** 具备四个能力布尔字段的模型/成员条目（结构化入参，两处调用方共用）。 */
	model: CapabilitySource;
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
