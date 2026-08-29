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

interface VirtualModelSectionProps {
	virtualModel: VirtualModel;
	onEdit: (virtualModel: VirtualModel) => void;
	onDelete: (virtualModel: VirtualModel) => void;
}

/** 成员卡片（纯展示）：模型 ID + 供应商名 + 能力图标；停用/随供应商禁用带标记。 */
function MemberCard({ item }: { item: VirtualModelItem }) {
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
					<span className="shrink-0 text-amber-600 dark:text-amber-400">· 随供应商禁用</span>
				)}
				{item.enable === false && <span className="shrink-0">· 已停用</span>}
			</p>
		</div>
	);
}

/** 单个虚拟模型区块：顶行左名称右菜单（编辑/删除），分割线下方平铺成员卡片。 */
export function VirtualModelSection({ virtualModel, onEdit, onDelete }: VirtualModelSectionProps) {
	return (
		<section>
			<div className="flex items-center justify-between gap-4 py-3">
				<div className="flex min-w-0 items-center gap-2">
					<h2 className="min-w-0 shrink-0 text-base font-semibold" title={virtualModel.displayId}>
						{virtualModel.displayId}
					</h2>
					{!virtualModel.enable && (
						<Badge variant="outline" className="shrink-0 text-muted-foreground">
							已禁用
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
							aria-label={`更多操作：${virtualModel.displayId}`}
						>
							<MoreHorizontal className="size-4" />
						</Button>
					</DropdownMenuTrigger>
					<DropdownMenuContent align="end">
						<DropdownMenuItem onClick={() => onEdit(virtualModel)}>
							<Pencil className="size-4" />
							编辑
						</DropdownMenuItem>
						<DropdownMenuSeparator />
						<DropdownMenuItem variant="destructive" onClick={() => onDelete(virtualModel)}>
							<Trash2 className="size-4" />
							删除
						</DropdownMenuItem>
					</DropdownMenuContent>
				</DropdownMenu>
			</div>
			<Separator />
			{virtualModel.items.length === 0 ? (
				<Card className="mt-4">
					<CardContent className="p-6 text-center text-sm text-muted-foreground">
						暂无成员模型，点击右上角菜单「编辑」添加
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
