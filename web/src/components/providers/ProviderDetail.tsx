import { EmptyState } from "@/components/empty-state";
import { ProviderProxyRow } from "@/components/providers/ProviderProxyRow";
import { ProviderUsageCard, usageEnabled } from "@/components/providers/ProviderUsageCard";
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
import { type Provider, fetchProviderApiKey, useUpdateProvider } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { cn } from "@/lib/utils";
import {
	ChevronRight,
	Copy,
	Eye,
	EyeOff,
	Gauge,
	KeyRound,
	MoreHorizontal,
	Pencil,
	Trash2,
} from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface ProviderDetailProps {
	provider: Provider | undefined;
	onEdit: (provider: Provider) => void;
	onDelete: (provider: Provider) => void;
	onSpeedTest: (provider: Provider) => void;
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

/** 详情页可折叠区：整行标题可点切换，右侧方向键指示状态（默认折叠），展开内容带轻量下滑动画。 */
function CollapsibleSection({ title, children }: { title: string; children: React.ReactNode }) {
	const [open, setOpen] = useState(false);
	return (
		<div>
			<button
				type="button"
				aria-expanded={open}
				onClick={() => setOpen((v) => !v)}
				className="flex w-full items-center justify-between gap-2 rounded-lg px-1 py-0.5 text-left transition-colors hover:bg-muted/60"
			>
				<span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
					{title}
				</span>
				<ChevronRight
					aria-hidden="true"
					className={cn(
						"size-4 shrink-0 text-muted-foreground transition-transform",
						open && "rotate-90",
					)}
				/>
			</button>
			{open && (
				<div className="mt-2 animate-in fade-in slide-in-from-top-2 duration-200">{children}</div>
			)}
		</div>
	);
}

export function ProviderDetail({ provider, onEdit, onDelete, onSpeedTest }: ProviderDetailProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateProvider = useUpdateProvider();
	// 明文仅本地展示用，不在任何缓存中保存；每次点开/复制都重新请求。
	const [plainKey, setPlainKey] = useState<string | null>(null);
	const [keyLoading, setKeyLoading] = useState(false);

	// 切换选择时重置明文展示状态。
	const activeId = provider?.id;
	const previousId = useRef(activeId);
	if (previousId.current !== activeId) {
		previousId.current = activeId;
		setPlainKey(null);
		setKeyLoading(false);
	}

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

	/** 点小眼睛：脱敏 → 请求明文展示；明文 → 本地切回脱敏（不发请求）。 */
	const handleToggleKey = async () => {
		if (plainKey !== null) {
			setPlainKey(null);
			return;
		}
		setKeyLoading(true);
		try {
			const key = await fetchProviderApiKey(provider.id);
			setPlainKey(key);
		} catch (error) {
			toastError(t("apiKeys.showKeyFailed"), error as Error);
		} finally {
			setKeyLoading(false);
		}
	};

	/** 一键复制：无论当前是否已展示明文，都重新请求明文后写入剪贴板。 */
	const handleCopyKey = async () => {
		try {
			const plain = await fetchProviderApiKey(provider.id);
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
						{keyLoading ? (
							<Skeleton className="h-5 w-40" />
						) : (
							<span className="flex items-center gap-2 font-mono">
								<KeyRound className="size-4 shrink-0 text-muted-foreground" />
								<span className="truncate">{plainKey ?? provider.apiKeyMasked}</span>
								<Button
									type="button"
									variant="ghost"
									size="icon"
									className="size-7 shrink-0"
									disabled={keyLoading}
									aria-label={plainKey ? t("apiKeys.hideKey") : t("apiKeys.showKey")}
									onClick={handleToggleKey}
								>
									{plainKey ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
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
					<DetailRow label={t("providers.proxyEnabled")}>
						<ProviderProxyRow enabled={provider.proxyEnabled} addr={provider.proxyAddr} />
					</DetailRow>
					<DetailRow label={t("providers.createdAt")}>{formatDate(provider.createdAt)}</DetailRow>
					<DetailRow label={t("providers.updatedAt")}>{formatDate(provider.updatedAt)}</DetailRow>
				</div>

				{usageEnabled(provider.extra) && <ProviderUsageCard providerId={provider.id} />}

				{provider.extra && provider.extra !== "{}" && (
					// key 按供应商 id：切换供应商时 remount，折叠态随之重置。
					<CollapsibleSection key={`extra-${provider.id}`} title={t("providers.extraConfig")}>
						<div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
							{Object.entries(JSON.parse(provider.extra) as Record<string, unknown>).map(
								([key, value]) => (
									<div key={key} className="space-y-1">
										<Label className="text-xs text-muted-foreground">{key}</Label>
										<Input readOnly value={String(value)} className="h-8 font-mono text-xs" />
									</div>
								),
							)}
						</div>
					</CollapsibleSection>
				)}

				{provider.customHeader && provider.customHeader !== "{}" && (
					<CollapsibleSection key={`header-${provider.id}`} title={t("providers.customHeader")}>
						<pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-lg bg-muted/50 p-3 font-mono text-xs">
							{JSON.stringify(JSON.parse(provider.customHeader), null, 2)}
						</pre>
					</CollapsibleSection>
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
							<DropdownMenuItem onClick={() => onSpeedTest(provider)}>
								<Gauge className="size-4" />
								{t("providers.speedTest")}
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
