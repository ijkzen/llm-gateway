import { EmptyState } from "@/components/empty-state";
import { TestFailedDialog } from "@/components/provider-models/TestFailedDialog";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import {
	type ProviderModel,
	useProviderModels,
	useTestProviderModel,
} from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import { Gauge, Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

interface ProviderSpeedTestDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	provider: Provider | null;
}

/** 每行测试结果：缺省=未测；数值=成功耗时（ms）；"failed"=失败（错误详情走失败弹窗）。 */
type ResultMap = Record<number, number | "failed">;

/** 供应商模型测速弹窗：逐行对该供应商名下模型发测试请求，成功显示耗时、失败弹窗展示错误。 */
export function ProviderSpeedTestDialog({
	open,
	onOpenChange,
	provider,
}: ProviderSpeedTestDialogProps) {
	const { t } = useTranslation();
	const { data: models, isLoading } = useProviderModels();
	const testModel = useTestProviderModel(provider?.id ?? 0);
	const [results, setResults] = useState<ResultMap>({});
	const [testingId, setTestingId] = useState<number | null>(null);
	const [activeError, setActiveError] = useState<string | null>(null);

	// 每次打开（含关闭后重开）都清空上一次的结果，重新开始。
	useEffect(() => {
		if (open) {
			setResults({});
			setTestingId(null);
			setActiveError(null);
		}
	}, [open]);

	const providerModels = useMemo(
		() => (models ?? []).filter((m) => provider !== null && m.providerId === provider.id),
		[models, provider],
	);

	const runTest = (model: ProviderModel) => {
		if (testModel.isPending) return;
		setTestingId(model.modelId);
		testModel.mutate(model.modelId, {
			onSuccess: (durationMs) => {
				setTestingId(null);
				setResults((prev) => ({ ...prev, [model.modelId]: durationMs }));
			},
			onError: (error) => {
				setTestingId(null);
				setResults((prev) => ({ ...prev, [model.modelId]: "failed" }));
				setActiveError(error.message);
			},
		});
	};

	return (
		<>
			<Dialog open={open} onOpenChange={onOpenChange}>
				<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[520px]">
					<DialogHeader className="space-y-3">
						<DialogTitle className="flex items-center gap-2">
							<Gauge className="size-4" />
							{t("providers.speedTest")}
						</DialogTitle>
						<DialogDescription>{t("providers.speedTestDesc")}</DialogDescription>
					</DialogHeader>

					{isLoading ? (
						<div className="space-y-2">
							<Skeleton className="h-10 w-full" />
							<Skeleton className="h-10 w-full" />
						</div>
					) : providerModels.length === 0 ? (
						<EmptyState
							title={t("providers.speedTestNoModels")}
							description={t("providers.speedTestNoModelsHint")}
						/>
					) : (
						<ul className="space-y-2">
							{providerModels.map((model) => {
								const result = results[model.modelId];
								const isTesting = testingId === model.modelId;
								return (
									<li
										key={model.modelId}
										className="flex items-center justify-between gap-3 rounded-lg border px-3 py-2.5"
									>
										<span
											className="min-w-0 truncate font-mono text-sm"
											title={model.providerModelId}
										>
											{model.providerModelId}
										</span>
										<span className="flex shrink-0 items-center gap-2">
											{typeof result === "number" && (
												<span className="text-sm text-muted-foreground tabular-nums">
													{t("providers.speedTestResult", { duration: result })}
												</span>
											)}
											{result === "failed" && (
												<span className="text-sm text-destructive">
													{t("providerModels.testFailedTitle")}
												</span>
											)}
											<Button
												type="button"
												variant="outline"
												size="sm"
												disabled={testModel.isPending}
												onClick={() => runTest(model)}
											>
												{isTesting ? (
													<Loader2 className="mr-1.5 size-3.5 animate-spin" />
												) : (
													<Gauge className="mr-1.5 size-3.5" />
												)}
												{t("providerModels.test")}
											</Button>
										</span>
									</li>
								);
							})}
						</ul>
					)}
				</DialogContent>
			</Dialog>

			<TestFailedDialog message={activeError} onClose={() => setActiveError(null)} />
		</>
	);
}
