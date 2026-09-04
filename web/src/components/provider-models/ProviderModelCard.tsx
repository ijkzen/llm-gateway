import { CapabilityIcons } from "@/components/provider-models/CapabilityIcons";
import type { ProviderModel } from "@/hooks/use-provider-models";
import { ArrowRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

interface ProviderModelCardProps {
	model: ProviderModel;
	onOpen: (model: ProviderModel) => void;
}

/** 模型卡片：左侧模型 ID 与方向键跳转数据面板，右侧能力图标打开详情弹窗。 */
export function ProviderModelCard({ model, onOpen }: ProviderModelCardProps) {
	const { t } = useTranslation();
	return (
		<div className="flex w-full items-center justify-between gap-3 rounded-xl border bg-card p-4 shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md">
			<Link
				to={`/models/${model.providerId}/${encodeURIComponent(model.providerModelId)}/overview`}
				className="group flex min-w-0 flex-1 items-center gap-1 rounded-md"
				title={t("providerModels.viewModelOverview", { model: model.providerModelId })}
			>
				<span className="min-w-0 flex-1 truncate text-sm font-medium">{model.providerModelId}</span>
				<ArrowRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
			</Link>
			<button
				type="button"
				onClick={() => onOpen(model)}
				aria-label={t("providerModels.viewDetails")}
				title={t("providerModels.viewDetails")}
				className="flex shrink-0 cursor-pointer items-center rounded-md p-0.5 transition-colors hover:bg-muted"
			>
				<CapabilityIcons model={model} />
			</button>
		</div>
	);
}
