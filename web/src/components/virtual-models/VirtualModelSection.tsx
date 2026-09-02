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
import { MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

interface VirtualModelSectionProps {
	virtualModel: VirtualModel;
	onEdit: (virtualModel: VirtualModel) => void;
	onDelete: (virtualModel: VirtualModel) => void;
}

/** 成员卡片（纯展示）：模型 ID + 供应商名 + 能力图标；停用/随供应商禁用带标记。 */
function MemberCard({ item }: { item: VirtualModelItem }) {
	const { t } = useTranslation();
	const providerDisabled = !item.providerEnable;
	return (
		<div
			className={cn(
				"rounded-xl border bg-card p-4 shadow-sm",
				(item.enable === false || providerDisabled) && "opacity-60",
			)}
		>
			<div className="flex items-center justify-between gap-3">
				<span
					className="min-w-0 truncate font-mono text-sm font-medium"
					title={item.providerModelId}
				>
					{item.providerModelId}
				</span>
				<ItemCapabilityIcons item={item} className="shrink-0" />
			</div>
			<p className="mt-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
				<span className="truncate">{item.providerName}</span>
				{providerDisabled && (
					<span className="shrink-0 text-warning">{t("virtualModels.disabledWithProvider")}</span>
				)}
				{item.enable === false && (
					<span className="shrink-0">{t("virtualModels.disabledMark")}</span>
				)}
			</p>
		</div>
	);
}

/** 单个虚拟模型区块：顶行左名称右菜单（编辑/删除），分割线下方平铺成员卡片。 */
export function VirtualModelSection({ virtualModel, onEdit, onDelete }: VirtualModelSectionProps) {
	const { t } = useTranslation();
	return (
		<section>
			<div className="flex items-center justify-between gap-4 py-3">
				<div className="flex min-w-0 items-center gap-2">
					<h2 className="min-w-0 shrink-0 text-base font-semibold" title={virtualModel.displayId}>
						{virtualModel.displayId}
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
						<MemberCard key={item.virtualModelItemId} item={item} />
					))}
				</div>
			)}
		</section>
	);
}
