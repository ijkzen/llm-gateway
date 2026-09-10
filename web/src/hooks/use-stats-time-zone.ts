import { useSettings } from "@/hooks/use-settings";
import { SETTING_KEY_TIMEZONE } from "@/i18n";

/** 设置表时区键（19-18：单源于 i18n/index，避免多处字面量漂移）。 */
export const STATS_TIME_ZONE_KEY = SETTING_KEY_TIMEZONE;
/** 缺省与后端 timezone_sync 一致（Asia/Shanghai）。 */
export const DEFAULT_STATS_TIME_ZONE = "Asia/Shanghai";

/**
 * 数据面板口径时区（IANA）：读设置表 timezone 行，缺省 Asia/Shanghai。
 * 数据面板的周期窗口与后端分桶都按该时区解释（管理后台单一时区视角）；
 * 设置变更后 react-query 使 data 更新，本 hook 返回新值驱动窗口重算。
 */
export function useStatsTimeZone(): string {
	const { data } = useSettings();
	return data?.find((s) => s.key === STATS_TIME_ZONE_KEY)?.value || DEFAULT_STATS_TIME_ZONE;
}
