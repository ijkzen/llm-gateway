import { EmptyState } from "@/components/empty-state";
import { MidEllipsis } from "@/components/mid-ellipsis";
import { billingModeLabel, protocolLabel } from "@/components/providers/ProtocolIcon";
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
import { useUsageEstimate } from "@/hooks/use-usage-estimate";
import { cn, formatDateTime, localeOf } from "@/lib/utils";
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
import { Link } from "react-router-dom";

interface ProviderDetailProps {
	provider: Provider | undefined;
	onEdit: (provider: Provider) => void;
	onDelete: (provider: Provider) => void;
	onSpeedTest: (provider: Provider) => void;
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
				className="flex w-full items-center justify-between gap-2 rounded-md px-1 py-0.5 text-left transition-colors hover:bg-muted/60"
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

/** 安全解析 JSON 对象文本（密钥缺失时后端可能透传密文）：失败返回 null。 */
function safeParseObject(text: string): Record<string, unknown> | null {
	try {
		const parsed: unknown = JSON.parse(text);
		return parsed && typeof parsed === "object" && !Array.isArray(parsed)
			? (parsed as Record<string, unknown>)
			: null;
	} catch {
		return null;
	}
}

export function ProviderDetail({ provider, onEdit, onDelete, onSpeedTest }: ProviderDetailProps) {
	const { t, i18n } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateProvider = useUpdateProvider();
	// 明文仅本地展示用，不在任何缓存中保存；每次点开/复制都重新请求。
	const [plainKey, setPlainKey] = useState<string | null>(null);
	const [keyLoading, setKeyLoading] = useState(false);

	// 切换选择时重置明文展示状态；activeIdRef 供在途请求比对丢弃迟到结果（17-01）。
	const activeId = provider?.id;
	const activeIdRef = useRef(activeId);
	activeIdRef.current = activeId;
	const previousId = useRef(activeId);
	if (previousId.current !== activeId) {
		previousId.current = activeId;
		setPlainKey(null);
		setKeyLoading(false);
	}

	// 订阅制 + 开启用量才拉取周期 Token 预估（非订阅制后端直接 400）。
	const canEstimate = provider?.billingMode === 1 && usageEnabled(provider?.extra ?? "");
	const usageEstimate = useUsageEstimate(canEstimate ? (provider?.id ?? null) : null);

	if (!provider) {
		return (
			<EmptyState
				title={t("providers.noProviderSelected")}
				description={t("providers.noProviderSelectedHint")}
			/>
		);
	}

	// 17-05：extra/customHeader 可能是后端透传的密文（非 JSON），解析失败即不渲染该块；
	// 空对象与旧行为一致地不渲染。
	const extraObject = safeParseObject(provider.extra);
	const hasExtra = extraObject !== null && Object.keys(extraObject).length > 0;
	const headerObject = safeParseObject(provider.customHeader);
	const hasHeader = headerObject !== null && Object.keys(headerObject).length > 0;

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
		const requestId = provider.id;
		setKeyLoading(true);
		try {
			const key = await fetchProviderApiKey(requestId);
			// 17-01：在途期间切换了供应商则丢弃结果，避免明文串号。
			if (activeIdRef.current !== requestId) return;
			setPlainKey(key);
		} catch (error) {
			if (activeIdRef.current !== requestId) return;
			toastError(t("common.loadFailed"), error);
		} finally {
			if (activeIdRef.current === requestId) {
				setKeyLoading(false);
			}
		}
	};

	/** 一键复制：无论当前是否已展示明文，都重新请求明文后写入剪贴板。 */
	const handleCopyKey = async () => {
		const requestId = provider.id;
		try {
			const plain = await fetchProviderApiKey(requestId);
			await navigator.clipboard.writeText(plain);
			toastSuccess(t("common.copiedToClipboard"));
		} catch (error) {
			if (activeIdRef.current !== requestId) return;
			toastError(t("common.copyFailed"), error);
		}
	};

	return (
		<Card className="flex flex-1 flex-col">
			<CardHeader className="border-b">
				<div className="flex items-start justify-between gap-4">
					<div className="min-w-0">
						<CardTitle className="text-xl">
							<Link
								to={`/providers/${provider.id}/overview`}
								className="group inline-flex max-w-full min-w-0 items-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60"
								title={t("providerModels.viewProviderOverview", {
									provider: provider.name,
								})}
							>
								<MidEllipsis text={provider.name} className="min-w-0" />
								<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
							</Link>
						</CardTitle>
						<MidEllipsis
							text={provider.baseUrl}
							className="mt-1 text-sm font-mono text-muted-foreground"
						/>
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
								<MidEllipsis text={plainKey ?? provider.apiKeyMasked} />
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
						{protocolLabel(provider.protocolType)}
					</DetailRow>
					<DetailRow label={t("providers.billingModeDetail")}>
						{billingModeLabel(provider.billingMode)}
					</DetailRow>
					<DetailRow label={t("providers.proxyEnabled")}>
						<ProviderProxyRow enabled={provider.proxyEnabled} addr={provider.proxyAddr} />
					</DetailRow>
					<DetailRow label={t("providers.createdAt")}>
						{formatDateTime(provider.createdAt, localeOf(i18n.language))}
					</DetailRow>
					<DetailRow label={t("providers.updatedAt")}>
						{formatDateTime(provider.updatedAt, localeOf(i18n.language))}
					</DetailRow>
				</div>

				{usageEnabled(provider.extra) && (
					<ProviderUsageCard providerId={provider.id} estimate={usageEstimate.data} />
				)}

				{hasExtra && (
					// key 按供应商 id：切换供应商时 remount，折叠态随之重置。
					<CollapsibleSection key={`extra-${provider.id}`} title={t("providers.extraConfig")}>
						<div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
							{Object.entries(extraObject).map(([key, value]) => (
								<div key={key} className="space-y-1">
									<Label className="text-xs text-muted-foreground">{key}</Label>
									<Input readOnly value={String(value)} className="h-8 font-mono text-xs" />
								</div>
							))}
						</div>
					</CollapsibleSection>
				)}

				{hasHeader && (
					<CollapsibleSection key={`header-${provider.id}`} title={t("providers.customHeader")}>
						<pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-muted/50 p-3 font-mono text-xs">
							{JSON.stringify(headerObject, null, 2)}
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
