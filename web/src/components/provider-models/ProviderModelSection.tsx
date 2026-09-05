import { MidEllipsis } from "@/components/mid-ellipsis";
import { ProviderModelCard } from "@/components/provider-models/ProviderModelCard";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import type { ProviderModel } from "@/hooks/use-provider-models";
import { type Provider, useUpdateProvider } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { ChevronRight, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

interface ProviderModelSectionProps {
	provider: Provider;
	models: ProviderModel[];
	onAdd: (provider: Provider) => void;
	onOpenModel: (provider: Provider, model: ProviderModel) => void;
}

/** 单个供应商区块：顶行左名称右启用开关与添加按钮，分割线下方平铺模型卡片。 */
export function ProviderModelSection({
	provider,
	models,
	onAdd,
	onOpenModel,
}: ProviderModelSectionProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateProvider = useUpdateProvider();
	const [enabled, setEnabled] = useState(provider.enable);

	useEffect(() => setEnabled(provider.enable), [provider.enable]);

	const toggleProvider = (next: boolean) => {
		setEnabled(next);
		updateProvider.mutate(
			{ id: provider.id, enable: next },
			{
				onSuccess: () => toastSuccess(t("common.updateSuccess")),
				onError: (error) => {
					setEnabled(provider.enable);
					toastError(t("common.updateFailed"), error);
				},
			},
		);
	};

	return (
		<section>
			<div className="flex items-center justify-between gap-4 py-3">
				<h2 className="min-w-0 text-base font-semibold">
					<Link
						to={`/providers/${provider.id}/overview`}
						className="group flex min-w-0 items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60"
						title={t("providerModels.viewProviderOverview", { provider: provider.name })}
					>
						<MidEllipsis text={provider.name} className="min-w-0" />
						<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
					</Link>
				</h2>
				<div className="flex shrink-0 items-center gap-3">
					<Switch
						checked={enabled}
						onCheckedChange={toggleProvider}
						aria-label={t("providerModels.toggleProvider", { provider: provider.name })}
					/>
					<Button size="sm" onClick={() => onAdd(provider)}>
						<Plus className="mr-2 size-4" />
						添加
					</Button>
				</div>
			</div>
			<Separator />
			{models.length === 0 ? (
				<Card className="mt-4">
					<CardContent className="p-6 text-center text-sm text-muted-foreground">
						{t("providerModels.emptyProviderModels")}
					</CardContent>
				</Card>
			) : (
				<div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
					{models.map((model) => (
						<ProviderModelCard
							key={model.modelId}
							model={model}
							onOpen={(selectedModel) => onOpenModel(provider, selectedModel)}
						/>
					))}
				</div>
			)}
		</section>
	);
}
