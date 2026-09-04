import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Separator } from "@/components/ui/separator";
import { ItemCapabilityIcons } from "@/components/virtual-models/ItemCapabilityIcons";
import type { VirtualModel, VirtualModelItem } from "@/hooks/use-virtual-models";
import { fallbackLabel, loadBalancingLabel } from "@/lib/constants";
import { cn } from "@/lib/utils";
import { ChevronRight, MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router-dom";

interface VirtualModelSectionProps {
	virtualModel: VirtualModel;
	onEdit: (virtualModel: VirtualModel) => void;
	onDelete: (virtualModel: VirtualModel) => void;
	/** 点击成员卡片打开只读详情弹窗（无论成员是否停用/随供应商禁用）。 */
	onOpenItem: (virtualModel: VirtualModel, item: VirtualModelItem) => void;
}

/** 成员卡片：模型 ID + 供应商名 + 能力图标；停用/随供应商禁用带标记；点击空白弹详情。 */
function MemberCard({
	item,
	onOpen,
}: {
	item: VirtualModelItem;
	onOpen: (item: VirtualModelItem) => void;
}) {
	const { t } = useTranslation();
	const navigate = useNavigate();
	const providerDisabled = !item.providerEnable;
	return (
		<button
			type="button"
			data-testid={`virtual-model-member-${item.virtualModelItemId}`}
			onClick={(event) => {
				const target = event.target as HTMLElement;
				if (target.closest("[data-nav]")) {
					navigate(
						`/models/${item.providerId}/${encodeURIComponent(item.providerModelId)}/overview`,
					);
					return;
				}
				if (target.closest("[data-static]")) return;
				onOpen(item);
			}}
			title={item.providerModelId}
			className={cn(
				"flex w-full cursor-pointer flex-col gap-1.5 rounded-xl border bg-card p-4 text-left shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md",
				(item.enable === false || providerDisabled) && "opacity-60",
			)}
		>
			<div className="flex items-center justify-between gap-3">
				<span
					data-nav
					className="group flex min-w-0 cursor-pointer items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60"
					title={t("providerModels.viewModelOverview", { model: item.providerModelId })}
				>
					<span className="min-w-0 max-w-full truncate font-mono text-sm font-medium">
						{item.providerModelId}
					</span>
					<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
				</span>
				<span data-static className="flex shrink-0 items-center">
					<ItemCapabilityIcons item={item} className="shrink-0" />
				</span>
			</div>
			<p className="flex items-center gap-1.5 text-xs text-muted-foreground">
				<span className="truncate">{item.providerName}</span>
				{providerDisabled && (
					<span className="shrink-0 text-warning">{t("virtualModels.disabledWithProvider")}</span>
				)}
				{item.enable === false && (
					<span className="shrink-0">{t("virtualModels.disabledMark")}</span>
				)}
			</p>
		</button>
	);
}

/** 单个虚拟模型区块：顶行左名称右菜单（编辑/删除），分割线下方平铺成员卡片。 */
export function VirtualModelSection({
	virtualModel,
	onEdit,
	onDelete,
	onOpenItem,
}: VirtualModelSectionProps) {
	const { t } = useTranslation();
	return (
		<section>
			<div className="flex items-center justify-between gap-4 py-3">
				<div className="flex min-w-0 items-center gap-2">
					<h2 className="min-w-0 shrink-0 text-base font-semibold">
						<Link
							to={`/virtual-models/${virtualModel.virtualModelId}/overview`}
							className="group flex min-w-0 items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60"
							title={t("virtualModels.viewVirtualModelOverview", {
								model: virtualModel.displayId,
							})}
						>
							<span className="min-w-0 truncate">{virtualModel.displayId}</span>
							<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
						</Link>
					</h2>
					{!virtualModel.enable && (
						<Badge variant="outline" className="shrink-0 text-muted-foreground">
							{t("common.disabled")}
						</Badge>
					)}
					<Badge variant="outline" className="shrink-0">
						{loadBalancingLabel(virtualModel.loadBalancingStrategy)}
					</Badge>
					<Badge variant="outline" className="shrink-0">
						{fallbackLabel(virtualModel.fallbackStrategy)}
					</Badge>
				</div>
				<DropdownMenu modal={false}>
					<DropdownMenuTrigger asChild>
						<Button
							variant="outline"
							size="icon"
							className="size-9 shrink-0"
							aria-label={`${t("common.moreActions")}：${virtualModel.displayId}`}
						>
							<MoreHorizontal className="size-4" />
						</Button>
					</DropdownMenuTrigger>
					<DropdownMenuContent align="end">
						<DropdownMenuItem onClick={() => onEdit(virtualModel)}>
							<Pencil className="size-4" />
							{t("common.edit")}
						</DropdownMenuItem>
						<DropdownMenuSeparator />
						<DropdownMenuItem variant="destructive" onClick={() => onDelete(virtualModel)}>
							<Trash2 className="size-4" />
							{t("common.delete")}
						</DropdownMenuItem>
					</DropdownMenuContent>
				</DropdownMenu>
			</div>
			<Separator />
			{virtualModel.items.length === 0 ? (
				<Card className="mt-4">
					<CardContent className="p-6 text-center text-sm text-muted-foreground">
						{t("virtualModels.noMemberHint")}
					</CardContent>
				</Card>
			) : (
				<div className="mt-4 grid grid-cols-1 gap-3 pb-6 sm:grid-cols-2 xl:grid-cols-3">
					{virtualModel.items.map((item) => (
						<MemberCard
							key={item.virtualModelItemId}
							item={item}
							onOpen={(selected) => onOpenItem(virtualModel, selected)}
						/>
					))}
				</div>
			)}
		</section>
	);
}
