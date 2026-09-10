import { MidEllipsis } from "@/components/mid-ellipsis";
import { CAPABILITIES } from "@/components/provider-models/CapabilityIcons";
import { TestFailedDialog } from "@/components/provider-models/TestFailedDialog";
import { ProviderProxyRow } from "@/components/providers/ProviderProxyRow";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { useTestProviderModel } from "@/hooks/use-provider-models";
import { useToastActions } from "@/hooks/use-toast";
import {
	type VirtualModel,
	type VirtualModelItem,
	useUpdateVirtualModel,
} from "@/hooks/use-virtual-models";
import { ChevronRight, FlaskConical, Loader2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

interface VirtualModelItemDetailDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	/** 被点击成员所属的虚拟模型（提供完整成员集合，供开关提交翻转后整体更新）。 */
	virtualModel: VirtualModel | null;
	/** 被查看详情的成员条目。 */
	item: VirtualModelItem | null;
}

/**
 * 成员模型详情（只读）：展示条目信息、状态标记与网络代理（模型级优先，
 * 供应商级继承），提供「测试」按钮验证上游连通性；另提供「在虚拟模型中
 * 启用」开关。开关成功后关闭弹窗（列表经查询失效重排），失败保持打开并报错。
 */
export function VirtualModelItemDetailDialog({
	open,
	onOpenChange,
	virtualModel,
	item,
}: VirtualModelItemDetailDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateModel = useUpdateVirtualModel();
	// 关闭/未选中时 providerId 兜底为 0：测试端点不会在该状态下被触发。
	const testModel = useTestProviderModel(item?.providerId ?? 0);
	const [testError, setTestError] = useState<string | null>(null);

	if (!open || !virtualModel || !item) return null;

	// 以虚拟模型成员集合为准定位当前条目（渲染快照，弹窗只读不依赖查询实时性）。
	const currentItem =
		virtualModel.items.find((i) => i.virtualModelItemId === item.virtualModelItemId) ?? item;
	const providerDisabled = !currentItem.providerEnable;

	// 开关提交用点击瞬间的成员集合快照。成功后关闭弹窗：让列表经查询失效
	// 重取最新成员序与启用位，避免弹窗继续渲染旧快照导致二次翻转错位。
	const toggle = (next: boolean) => {
		const items = virtualModel.items.map((i) => ({
			modelId: i.modelId,
			enable: i.virtualModelItemId === currentItem.virtualModelItemId ? next : i.enable,
		}));
		updateModel.mutate(
			{ id: virtualModel.virtualModelId, items },
			{
				onSuccess: () => {
					onOpenChange(false);
					toastSuccess(t("common.updateSuccess"));
				},
				onError: (error) => toastError(t("common.updateFailed"), error),
			},
		);
	};

	const handleTest = () => {
		if (testModel.isPending) return;
		testModel.mutate(currentItem.modelId, {
			onSuccess: () => toastSuccess(t("providerModels.testSuccess")),
			onError: (error) => setTestError(error.message),
		});
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			{/* 17-19：固定头/尾 + 仅中间主体滚动的三分布局（与同族大弹窗一致）。 */}
			<DialogContent className="flex h-[min(720px,85vh)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[520px]">
				<DialogHeader className="shrink-0 space-y-3 px-6 pb-4 pt-6">
					<DialogTitle className="min-w-0">
						<Link
							to={`/models/${currentItem.modelId}/overview`}
							className="group inline-flex max-w-full min-w-0 items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60"
							title={t("providerModels.viewModelOverview", {
								model: currentItem.providerModelId,
							})}
						>
							<MidEllipsis text={currentItem.providerModelId} className="min-w-0" />
							<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
						</Link>
					</DialogTitle>
					<DialogDescription>
						{t("providerModels.belongsToProvider")}
						{currentItem.providerName}
						{providerDisabled && (
							<span className="ml-1.5 text-warning">{t("virtualModels.disabledWithProvider")}</span>
						)}
						{currentItem.enable === false && (
							<span className="ml-1.5">{t("virtualModels.disabledMark")}</span>
						)}
					</DialogDescription>
				</DialogHeader>

				<div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
					<dl className="space-y-3">
						<div className="flex items-center justify-between gap-4 rounded-lg border px-4 py-2.5">
							<dt className="text-sm text-muted-foreground">{t("providerModels.modelId")}</dt>
							<dd className="min-w-0 font-mono text-sm">
								<MidEllipsis text={currentItem.providerModelId} />
							</dd>
						</div>
						<div className="grid grid-cols-2 gap-3">
							<div className="rounded-lg border px-4 py-2.5">
								<dt className="text-xs text-muted-foreground">
									{t("providerModels.contextLength")}
								</dt>
								<dd className="mt-0.5 text-sm font-medium">
									{currentItem.contextLength.toLocaleString()}
								</dd>
							</div>
							<div className="rounded-lg border px-4 py-2.5">
								<dt className="text-xs text-muted-foreground">{t("providerModels.maxOutput")}</dt>
								<dd className="mt-0.5 text-sm font-medium">
									{currentItem.maxOutputTokens.toLocaleString()}
								</dd>
							</div>
						</div>
						<div className="rounded-lg border px-4 py-3">
							<dt className="text-xs text-muted-foreground">
								{t("providerModels.modelCapabilities")}
							</dt>
							<dd className="mt-2 grid grid-cols-2 gap-2">
								{CAPABILITIES.map(({ key, labelKey, icon: Icon }) => (
									<span
										key={key}
										className={
											currentItem[key]
												? "flex items-center gap-1.5 text-sm text-success"
												: "flex items-center gap-1.5 text-sm text-muted-foreground/60"
										}
									>
										<Icon className="size-3.5" />
										{t(labelKey)}
										{currentItem[key]
											? t("providerModels.supported")
											: t("providerModels.notSupported")}
									</span>
								))}
							</dd>
						</div>
						<div className="flex items-center justify-between gap-4 rounded-lg border px-4 py-2.5">
							<dt className="text-sm text-muted-foreground">{t("providers.proxyEnabled")}</dt>
							<dd>
								<ProviderProxyRow
									enabled={currentItem.modelProxyEnabled}
									addr={currentItem.modelProxyAddr}
									inherited={
										currentItem.providerProxyEnabled ? currentItem.providerProxyAddr : undefined
									}
								/>
							</dd>
						</div>
						<div className="flex items-center justify-between rounded-lg border px-4 py-2.5">
							<dt className="text-sm text-muted-foreground">
								{t("virtualModels.enableInVirtualModel")}
							</dt>
							<dd>
								<Switch
									checked={currentItem.enable}
									disabled={updateModel.isPending}
									onCheckedChange={toggle}
									aria-label={`${t("virtualModels.enableInVirtualModel")} ${currentItem.providerModelId}`}
								/>
							</dd>
						</div>
					</dl>
				</div>

				<DialogFooter className="shrink-0 gap-2 border-t px-6 py-4">
					<Button
						type="button"
						variant="outline"
						size="sm"
						onClick={handleTest}
						disabled={testModel.isPending}
					>
						{testModel.isPending ? (
							<Loader2 className="mr-1.5 size-4 animate-spin" />
						) : (
							<FlaskConical className="mr-1.5 size-4" />
						)}
						{t(testModel.isPending ? "providerModels.testing" : "providerModels.test")}
					</Button>
				</DialogFooter>
			</DialogContent>

			<TestFailedDialog message={testError} onClose={() => setTestError(null)} />
		</Dialog>
	);
}
