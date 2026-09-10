import { MidEllipsis } from "@/components/mid-ellipsis";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { type ApiKey, fetchApiKeyDetail, useApiKeyDetail } from "@/hooks/use-api-keys";
import { useToastActions } from "@/hooks/use-toast";
import { Copy, Eye, EyeOff, KeyRound } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface ApiKeyCellProps {
	apiKey: ApiKey;
}

/** 表格中的 Key 单元格：默认展示掩码，小眼睛按需拉取明文，支持一键复制。 */
export function ApiKeyCell({ apiKey }: ApiKeyCellProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const [showKey, setShowKey] = useState(false);

	// 仅在展示明文时拉取详情，避免列表加载即解密全部密钥。
	const {
		data: detail,
		isLoading: detailLoading,
		isError,
		error,
	} = useApiKeyDetail(showKey ? apiKey.id : null);
	const effectiveShowKey = showKey && !detailLoading && detail?.id === apiKey.id;

	// 17-09：拉取失败此前只静默停在掩码，用户会以为按钮失灵；补一次错误提示。
	const reportedErrorId = useRef<number | null>(null);
	useEffect(() => {
		if (!isError || reportedErrorId.current === apiKey.id) return;
		reportedErrorId.current = apiKey.id;
		toastError(t("common.loadFailed"), error as Error);
	}, [isError, error, apiKey.id, t, toastError]);

	const handleCopy = async () => {
		try {
			// 已展开的用已加载明文；否则直接请求（17-10：明文不进任何查询缓存）。
			const plain =
				effectiveShowKey && detail
					? detail.key
					: await fetchApiKeyDetail(apiKey.id).then((d) => d.key);
			await navigator.clipboard.writeText(plain);
			toastSuccess(t("common.copiedToClipboard"));
		} catch (error) {
			toastError(t("common.copyFailed"), error as Error);
		}
	};

	return (
		<span className="flex items-center gap-1.5 font-mono">
			<KeyRound className="size-4 shrink-0 text-muted-foreground" />
			{showKey && detailLoading ? (
				<Skeleton className="h-5 w-40" />
			) : (
				<MidEllipsis
					className="max-w-[24rem]"
					text={effectiveShowKey ? (detail?.key ?? "") : apiKey.keyMasked}
				/>
			)}
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
				onClick={handleCopy}
			>
				<Copy className="size-4" />
			</Button>
		</span>
	);
}
