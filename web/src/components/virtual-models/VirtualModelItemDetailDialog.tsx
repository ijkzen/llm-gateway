import { CAPABILITIES } from "@/components/provider-models/CapabilityIcons";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { useToastActions } from "@/hooks/use-toast";
import {
	type VirtualModel,
	type VirtualModelItem,
	useUpdateVirtualModel,
} from "@/hooks/use-virtual-models";
import { useTranslation } from "react-i18next";

interface VirtualModelItemDetailDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	/** 被点击成员所属的虚拟模型（提供完整成员集合，供开关提交翻转后整体更新）。 */
	virtualModel: VirtualModel | null;
	/** 被查看详情的成员条目。 */
	item: VirtualModelItem | null;
}

/**
 * 成员模型详情（只读）：展示条目信息与状态标记，并提供「在虚拟模型中启用」开关。
 * 开关成功后关闭弹窗（列表经查询失效重排），失败保持打开并报错。
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

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[520px]">
				<DialogHeader className="space-y-3">
					<DialogTitle className="truncate" title={currentItem.providerModelId}>
						{currentItem.providerModelId}
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

				<dl className="space-y-3">
					<div className="flex items-center justify-between gap-4 rounded-lg border px-4 py-2.5">
						<dt className="text-sm text-muted-foreground">{t("providerModels.modelId")}</dt>
						<dd className="min-w-0 truncate font-mono text-sm">{currentItem.providerModelId}</dd>
					</div>
					<div className="grid grid-cols-2 gap-3">
						<div className="rounded-lg border px-4 py-2.5">
							<dt className="text-xs text-muted-foreground">{t("providerModels.contextLength")}</dt>
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
			</DialogContent>
		</Dialog>
	);
}
