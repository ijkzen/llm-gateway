import { CapabilityIcons } from "@/components/provider-models/CapabilityIcons";
import type { ProviderModel } from "@/hooks/use-provider-models";

interface ProviderModelCardProps {
	model: ProviderModel;
	onOpen: (model: ProviderModel) => void;
}

/** 模型卡片：展示模型 ID 与能力图标，点击打开详情弹窗。 */
export function ProviderModelCard({ model, onOpen }: ProviderModelCardProps) {
	return (
		<button
			type="button"
			onClick={() => onOpen(model)}
			title={model.providerModelId}
			className="flex w-full cursor-pointer items-center justify-between gap-3 rounded-xl border bg-card p-4 text-left shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md"
		>
			<span className="min-w-0 flex-1 truncate text-sm font-medium">{model.providerModelId}</span>
			<CapabilityIcons model={model} className="shrink-0" />
		</button>
	);
}
