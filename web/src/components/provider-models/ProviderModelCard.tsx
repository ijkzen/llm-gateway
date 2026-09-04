import { CapabilityIcons } from "@/components/provider-models/CapabilityIcons";
import type { ProviderModel } from "@/hooks/use-provider-models";
import { ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

interface ProviderModelCardProps {
	model: ProviderModel;
	onOpen: (model: ProviderModel) => void;
}

/** 模型卡片：点击卡片空白处打开详情弹窗；模型 ID + 方向键区域编程跳转数据面板。 */
export function ProviderModelCard({ model, onOpen }: ProviderModelCardProps) {
	const { t } = useTranslation();
	const navigate = useNavigate();
	return (
		<button
			type="button"
			data-testid={`provider-model-card-${model.modelId}`}
			className="flex w-full cursor-pointer items-center justify-between gap-3 rounded-xl border bg-card p-4 text-left shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md"
			onClick={(event) => {
				const target = event.target as HTMLElement;
				if (target.closest("[data-nav]")) {
					navigate(
						`/models/${model.providerId}/${encodeURIComponent(model.providerModelId)}/overview`,
					);
					return;
				}
				if (target.closest("[data-static]")) return;
				onOpen(model);
			}}
		>
			<span
				data-nav
				className="group flex min-w-0 max-w-full items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60"
				title={t("providerModels.viewModelOverview", { model: model.providerModelId })}
			>
				<span className="min-w-0 max-w-full truncate text-sm font-medium">
					{model.providerModelId}
				</span>
				<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
			</span>
			<span data-static className="flex shrink-0 items-center">
				<CapabilityIcons model={model} />
			</span>
		</button>
	);
}
