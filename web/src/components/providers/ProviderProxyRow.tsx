import { MidEllipsis } from "@/components/mid-ellipsis";
import { Badge } from "@/components/ui/badge";
import { useTranslation } from "react-i18next";

interface ProviderProxyRowProps {
	/** 本层（模型级/供应商级）是否开启代理。 */
	enabled: boolean;
	/** 本层代理地址（enabled 时展示）。 */
	addr?: string;
	/** 继承自上一层的代理（模型级关闭但供应商级开启时展示，避免误判为直连）。 */
	inherited?: string;
}

/** 网络代理只读展示行：开启=徽标+地址；关闭但上层有代理=「未开启·继承上层代理」；否则灰徽标。 */
export function ProviderProxyRow({ enabled, addr, inherited }: ProviderProxyRowProps) {
	const { t } = useTranslation();
	if (enabled) {
		return (
			<span className="inline-flex items-center gap-1.5">
				<Badge variant="default">{t("providers.proxyOn")}</Badge>
				<MidEllipsis
					text={addr ?? ""}
					className="max-w-48 font-mono text-xs text-muted-foreground"
				/>
			</span>
		);
	}
	if (inherited) {
		return (
			<span className="inline-flex items-center gap-1.5">
				<Badge variant="secondary">{t("providers.proxyOff")}</Badge>
				<MidEllipsis
					text={t("providers.proxyInherited")}
					className="max-w-48 text-xs text-muted-foreground"
				/>
			</span>
		);
	}
	return <Badge variant="secondary">{t("providers.proxyOff")}</Badge>;
}
