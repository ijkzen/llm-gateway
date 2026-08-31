import { CAPABILITIES } from "@/components/provider-models/CapabilityIcons";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { useTranslation } from "react-i18next";

interface ItemCapabilityIconsProps {
	/** 具备 reasoning/toolUse/imageUnderstand/videoUnderstand 布尔字段的条目。 */
	item: {
		reasoning: boolean;
		toolUse: boolean;
		imageUnderstand: boolean;
		videoUnderstand: boolean;
	};
	className?: string;
}

/** 成员能力图标：仅展示为 true 的项（tooltip 说明含义）。 */
export function ItemCapabilityIcons({ item, className }: ItemCapabilityIconsProps) {
	const { t } = useTranslation();
	return (
		<TooltipProvider delayDuration={200}>
			<div className={`flex items-center gap-1 ${className ?? ""}`}>
				{CAPABILITIES.map(({ key, labelKey, icon: Icon }) => {
					const label = t(labelKey);
					return item[key] ? (
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
					) : null;
				})}
			</div>
		</TooltipProvider>
	);
}
