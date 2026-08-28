import { EmptyState } from "@/components/empty-state";
import { StatusBadge } from "@/components/status-badge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { type Provider, useProviderDetail, useUpdateProvider } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { Eye, EyeOff, KeyRound, MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import { useRef, useState } from "react";

interface ProviderDetailProps {
	provider: Provider | undefined;
	onEdit: (provider: Provider) => void;
	onDelete: (provider: Provider) => void;
}

const PROTOCOL_LABELS: Record<number, string> = {
	0: "OpenAI Compatible",
	1: "OpenAI Response",
	2: "Anthropic",
	3: "Gemini",
};

const BILLING_LABELS: Record<number, string> = {
	0: "按量付费",
	1: "订阅制",
};

function formatDate(dateStr: string) {
	if (!dateStr) return "—";
	const ts = new Date(dateStr).getTime();
	if (Number.isNaN(ts) || ts <= 0) return "—";
	return new Date(dateStr).toLocaleString("zh-CN");
}

/** 详情字段网格中的一行。 */
function DetailRow({ label, children }: { label: string; children: React.ReactNode }) {
	return (
		<div>
			<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">{label}</p>
			<div className="mt-1 text-sm">{children}</div>
		</div>
	);
}

export function ProviderDetail({ provider, onEdit, onDelete }: ProviderDetailProps) {
	const { toastSuccess, toastError } = useToastActions();
	const updateProvider = useUpdateProvider();
	const [showKey, setShowKey] = useState(false);

	const { data: detail, isLoading: detailLoading } = useProviderDetail(provider?.id ?? null);

	// 切换选择时重置明文展示状态。
	const activeId = provider?.id;
	const previousId = useRef(activeId);
	if (previousId.current !== activeId) {
		previousId.current = activeId;
		setShowKey(false);
	}
	const effectiveShowKey =
		showKey && activeId !== null && detail?.id === activeId && !detailLoading;

	if (!provider) {
		return <EmptyState title="未选择 Provider" description="在左侧选择一个 Provider 查看详情" />;
	}

	const toggleEnable = () => {
		updateProvider.mutate(
			{ id: provider.id, enable: !provider.enable },
			{
				onSuccess: () => toastSuccess("操作成功"),
				onError: (error) => toastError("操作失败", error),
			},
		);
	};

	return (
		<Card className="flex flex-1 flex-col">
			<CardHeader className="border-b">
				<div className="flex items-start justify-between gap-4">
					<div className="min-w-0">
						<CardTitle className="text-xl">{provider.name}</CardTitle>
						<p className="mt-1 truncate text-sm font-mono text-muted-foreground">
							{provider.baseUrl}
						</p>
					</div>
					<div className="flex shrink-0 items-center gap-2">
						<Switch
							checked={provider.enable}
							disabled={updateProvider.isPending}
							aria-label={`切换 Provider ${provider.name} 状态`}
							onCheckedChange={toggleEnable}
						/>
						<StatusBadge status={provider.enable ? "enabled" : "disabled"} />
					</div>
				</div>
			</CardHeader>
			<CardContent className="flex-1 space-y-6 py-6">
				<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
					<DetailRow label="API Key">
						{detailLoading ? (
							<Skeleton className="h-5 w-40" />
						) : (
							<span className="flex items-center gap-2 font-mono">
								<KeyRound className="size-4 shrink-0 text-muted-foreground" />
								<span className="truncate">
									{effectiveShowKey ? detail?.apiKey : provider.apiKeyMasked}
								</span>
								<Button
									type="button"
									variant="ghost"
									size="icon"
									className="size-7 shrink-0"
									aria-label={effectiveShowKey ? "隐藏 API Key" : "显示 API Key"}
									onClick={() => setShowKey((v) => !v)}
								>
									{effectiveShowKey ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
								</Button>
							</span>
						)}
					</DetailRow>
					<DetailRow label="协议类型">{PROTOCOL_LABELS[provider.protocolType] ?? "未知"}</DetailRow>
					<DetailRow label="付费模式">{BILLING_LABELS[provider.billingMode] ?? "未知"}</DetailRow>
					<DetailRow label="状态">
						<Badge variant="outline">{provider.status === 0 ? "可用" : "不可用"}</Badge>
					</DetailRow>
					<DetailRow label="创建时间">{formatDate(provider.createdAt)}</DetailRow>
					<DetailRow label="更新时间">{formatDate(provider.updatedAt)}</DetailRow>
				</div>

				{provider.extra && provider.extra !== "{}" && (
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							额外配置
						</p>
						<pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-lg bg-muted/50 p-3 font-mono text-xs">
							{JSON.stringify(JSON.parse(provider.extra), null, 2)}
						</pre>
					</div>
				)}

				{provider.customHeader && provider.customHeader !== "{}" && (
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							自定义请求头
						</p>
						<pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-lg bg-muted/50 p-3 font-mono text-xs">
							{JSON.stringify(JSON.parse(provider.customHeader), null, 2)}
						</pre>
					</div>
				)}

				<div className="flex items-center gap-2 pt-4">
					<DropdownMenu modal={false}>
						<DropdownMenuTrigger asChild>
							<Button variant="outline" size="icon" className="size-9" aria-label="更多操作">
								<MoreHorizontal className="size-4" />
							</Button>
						</DropdownMenuTrigger>
						<DropdownMenuContent align="start">
							<DropdownMenuItem onClick={() => onEdit(provider)}>
								<Pencil className="size-4" />
								编辑
							</DropdownMenuItem>
							<DropdownMenuSeparator />
							<DropdownMenuItem variant="destructive" onClick={() => onDelete(provider)}>
								<Trash2 className="size-4" />
								删除
							</DropdownMenuItem>
						</DropdownMenuContent>
					</DropdownMenu>
				</div>
			</CardContent>
		</Card>
	);
}
