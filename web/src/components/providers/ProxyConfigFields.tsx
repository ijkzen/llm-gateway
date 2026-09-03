import {
	FormControl,
	FormDescription,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { Control, FieldValues, Path } from "react-hook-form";
import { useWatch } from "react-hook-form";
import { useTranslation } from "react-i18next";
interface ProxyConfigFieldsProps<T extends FieldValues> {
	control: Control<T>;
	/** 表单中代理开关字段名（默认 proxyEnabled）。 */
	enabledName?: Path<T>;
	/** 表单中代理地址字段名（默认 proxyAddr）。 */
	addrName?: Path<T>;
	/** 是否显示地址下方的提示文案（供应商弹窗在高级区常驻时展示）。 */
	withHint?: boolean;
}

/** 网络代理配置编辑块（开关 + 条件显示地址输入）。供应商与供应商模型弹窗共用，
 *  校验规则一致：开启时地址必填且 http:// 开头。 */
export function ProxyConfigFields<T extends FieldValues>({
	control,
	enabledName = "proxyEnabled" as Path<T>,
	addrName = "proxyAddr" as Path<T>,
	withHint = false,
}: ProxyConfigFieldsProps<T>) {
	const { t } = useTranslation();
	return (
		<>
			<FormField
				control={control}
				name={enabledName}
				render={({ field }) => (
					<FormItem className="flex items-center justify-between rounded-lg border p-3">
						<FormLabel>{t("providers.proxyEnabled")}</FormLabel>
						<FormControl>
							<Switch checked={field.value} onCheckedChange={field.onChange} />
						</FormControl>
					</FormItem>
				)}
			/>
			{/* 仅开启时显示地址输入（校验见各表单 schema superRefine）。 */}
			<ProxyAddrField
				control={control}
				name={addrName}
				enabledName={enabledName}
				withHint={withHint}
			/>
		</>
	);
}

/** 地址输入：跟随开关显隐。拆出便于在开关与输入之间插入其他内容时复用。 */
function ProxyAddrField<T extends FieldValues>({
	control,
	name,
	enabledName,
	withHint,
}: {
	control: Control<T>;
	name: Path<T>;
	enabledName: Path<T>;
	withHint: boolean;
}) {
	const { t } = useTranslation();
	const enabled = useWatch({ control, name: enabledName });
	if (!enabled) return null;
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem>
					<FormLabel>{t("providers.proxyAddr")}</FormLabel>
					<FormControl>
						<Input {...field} placeholder="http://127.0.0.1:7890" className="font-mono" />
					</FormControl>
					{withHint && <FormDescription>{t("providers.proxyAddrHint")}</FormDescription>}
					<FormMessage />
				</FormItem>
			)}
		/>
	);
}
