import { type Locale, SETTING_KEY_LANGUAGE, SETTING_KEY_TIMEZONE } from "@/i18n";
import { api } from "@/lib/api";

/** 浏览器时区（IANA 名，如 Asia/Shanghai）。 */
export function browserTimezone(): string {
	return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

/** 按 IANA 名生成时区选项：`(UTC+08:00) Asia/Shanghai`。 */
export function timezoneOptions(): { value: string; label: string }[] {
	const zones = Intl.supportedValuesOf("timeZone");
	return zones.map((zone) => {
		let offsetLabel = "UTC";
		try {
			const formatter = new Intl.DateTimeFormat("en-US", {
				timeZone: zone,
				timeZoneName: "shortOffset",
			});
			const parts = formatter.formatToParts(new Date());
			const name = parts.find((p) => p.type === "timeZoneName")?.value;
			if (name) offsetLabel = name;
		} catch {
			// 未知时区：仅显示 IANA 名。
		}
		return { value: zone, label: `(${offsetLabel}) ${zone}` };
	});
}

/**
 * 初始化完成后把引导页选择的语言与时区写入设置表。
 * 失败不阻塞登录跳转（返回是否全部成功，供提示用）。
 */
export async function saveInitSettings(
	locale: Locale,
	timezone: string,
): Promise<{ ok: boolean; failed: string[] }> {
	const failed: string[] = [];
	for (const [key, value] of [
		[SETTING_KEY_LANGUAGE, locale],
		[SETTING_KEY_TIMEZONE, timezone],
	] as const) {
		try {
			await api.put(`settings/${key}`, { json: { value } });
		} catch {
			failed.push(key);
		}
	}
	return { ok: failed.length === 0, failed };
}
