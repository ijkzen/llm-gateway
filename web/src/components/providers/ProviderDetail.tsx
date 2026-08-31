import { EmptyState } from "@/components/empty-state";
import { ProviderUsageCard, usageEnabled } from "@/components/providers/ProviderUsageCard";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import {
	type Provider,
	type ProviderDetail as ProviderDetailData,
	providerKeys,
	useProviderDetail,
	useUpdateProvider,
} from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQueryClient } from "@tanstack/react-query";
import { Copy, Eye, EyeOff, KeyRound, MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface ProviderDetailProps {
	provider: Provider | undefined;
	onEdit: (provider: Provider) => void;
	onDelete: (provider: Provider) => void;
}

const PROTOCOL_LABELS: Record<number, string> = {
	0: "providers.protocol.openaiCompat",
	1: "providers.protocol.responses",
	2: "providers.protocol.anthropic",
	3: "providers.protocol.gemini",
};

const BILLING_LABELS: Record<number, string> = {
	0: "providers.payAsYouGo",
	1: "providers.subscription",
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
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateProvider = useUpdateProvider();
	const queryClient = useQueryClient();
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
		return (
			<EmptyState
				title={t("providers.noProviderSelected")}
				description={t("providers.noProviderSelectedHint")}
			/>
		);
	}

	const toggleEnable = () => {
		updateProvider.mutate(
			{ id: provider.id, enable: !provider.enable },
			{
				onSuccess: () => toastSuccess(t("common.success")),
				onError: (error) => toastError(t("common.error"), error),
			},
		);
	};

	const handleCopyKey = async () => {
		if (!provider) return;
		try {
			// 已展开的用已加载明文，否则命令式拉取详情。
			const plain =
				effectiveShowKey && detail
					? detail.apiKey
					: (
							await queryClient.fetchQuery({
								queryKey: providerKeys.detail(provider.id),
								queryFn: async () => {
									const res = await api
										.get(`providers/${provider.id}`)
										.json<ApiResponse<ProviderDetailData>>();
									return unwrap(res);
								},
							})
						).apiKey;
			await navigator.clipboard.writeText(plain);
			toastSuccess(t("common.copiedToClipboard"));
		} catch (error) {
			toastError(t("common.copyFailed"), error as Error);
		}
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
							aria-label={`${t("providers.toggleProviderStatus")} ${provider.name} ${t("cronJobs.toggleStatusSuffix")}`}
							onCheckedChange={toggleEnable}
						/>
					</div>
				</div>
			</CardHeader>
			<CardContent className="flex-1 space-y-6 py-6">
				<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
					<DetailRow label={t("providers.apiKey")}>
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
									aria-label={effectiveShowKey ? t("apiKeys.hideKey") : t("apiKeys.showKey")}
									onClick={() => setShowKey((v) => !v)}
								>
									{effectiveShowKey ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
								</Button>
								<Button
									type="button"
									variant="ghost"
									size="icon"
									className="size-7 shrink-0"
									aria-label={t("apiKeys.copyKey")}
									onClick={handleCopyKey}
								>
									<Copy className="size-4" />
								</Button>
							</span>
						)}
					</DetailRow>
					<DetailRow label={t("providers.protocolType")}>
						{t(PROTOCOL_LABELS[provider.protocolType] ?? "common.unknown")}
					</DetailRow>
					<DetailRow label={t("providers.billingModeDetail")}>
						{t(BILLING_LABELS[provider.billingMode] ?? "common.unknown")}
					</DetailRow>
					<DetailRow label={t("providers.status")}>
						<Badge variant="outline">
							{provider.status === 0 ? t("providers.available") : t("providers.unavailable")}
						</Badge>
					</DetailRow>
					<DetailRow label={t("providers.createdAt")}>{formatDate(provider.createdAt)}</DetailRow>
					<DetailRow label={t("providers.updatedAt")}>{formatDate(provider.updatedAt)}</DetailRow>
				</div>

				{usageEnabled(provider.extra) && <ProviderUsageCard providerId={provider.id} />}

				{provider.extra && provider.extra !== "{}" && (
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							{t("providers.extraConfig")}
						</p>
						<div className="mt-1 grid grid-cols-1 gap-2 sm:grid-cols-2">
							{Object.entries(JSON.parse(provider.extra) as Record<string, unknown>).map(
								([key, value]) => (
									<div key={key} className="space-y-1">
										<Label className="text-xs text-muted-foreground">{key}</Label>
										<Input readOnly value={String(value)} className="h-8 font-mono text-xs" />
									</div>
								),
							)}
						</div>
					</div>
				)}

				{provider.customHeader && provider.customHeader !== "{}" && (
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							{t("providers.customHeader")}
						</p>
						<pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-lg bg-muted/50 p-3 font-mono text-xs">
							{JSON.stringify(JSON.parse(provider.customHeader), null, 2)}
						</pre>
					</div>
				)}

				<div className="flex items-center gap-2 pt-4">
					<DropdownMenu modal={false}>
						<DropdownMenuTrigger asChild>
							<Button
								variant="outline"
								size="icon"
								className="size-9"
								aria-label={t("common.moreActions")}
							>
								<MoreHorizontal className="size-4" />
							</Button>
						</DropdownMenuTrigger>
						<DropdownMenuContent align="start">
							<DropdownMenuItem onClick={() => onEdit(provider)}>
								<Pencil className="size-4" />
								{t("providers.edit")}
							</DropdownMenuItem>
							<DropdownMenuSeparator />
							<DropdownMenuItem variant="destructive" onClick={() => onDelete(provider)}>
								<Trash2 className="size-4" />
								{t("providers.delete")}
							</DropdownMenuItem>
						</DropdownMenuContent>
					</DropdownMenu>
				</div>
			</CardContent>
		</Card>
	);
}
