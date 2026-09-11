import { CAPABILITIES } from "@/components/provider-models/CapabilityIcons";
import { FormControl, FormField, FormItem, FormLabel } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import type { Control, Path } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

/** 正整数字段（上下文长度/最大输出共用，含校验文案）。 */
export function positiveIntField(t: (key: string) => string) {
	return z.coerce
		.number()
		.int(t("providerModels.mustBeInt"))
		.positive(t("providerModels.mustBePositive"));
}

/** 供应商模型基础字段 schema：模型 ID + 上下文/最大输出 + 四能力开关。 */
export function makeProviderModelBaseSchema(t: (key: string) => string) {
	return z.object({
		providerModelId: z.string().min(1, t("providerModels.modelIdRequired")),
		contextLength: positiveIntField(t),
		maxOutputTokens: positiveIntField(t),
		reasoning: z.boolean(),
		toolUse: z.boolean(),
		imageUnderstand: z.boolean(),
		videoUnderstand: z.boolean(),
	});
}

type CapabilityKey = (typeof CAPABILITIES)[number]["key"];

/** 四能力开关网格（react-hook-form FormField 版，添加/编辑弹窗共用）。 */
export function CapabilitySwitchGrid<TValues extends Record<CapabilityKey, boolean>>({
	control,
}: {
	control: Control<TValues>;
}) {
	const { t } = useTranslation();
	return (
		<div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
			{CAPABILITIES.map(({ key, labelKey }) => (
				<FormField
					key={key}
					control={control}
					name={key as Path<TValues>}
					render={({ field }) => (
						<FormItem className="flex items-center justify-between rounded-md border p-3">
							<FormLabel>{t(labelKey)}</FormLabel>
							<FormControl>
								<Switch checked={field.value} onCheckedChange={field.onChange} />
							</FormControl>
						</FormItem>
					)}
				/>
			))}
		</div>
	);
}
