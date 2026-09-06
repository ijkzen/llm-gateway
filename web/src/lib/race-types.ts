/** 赛马/指标查询共享类型（原在 6 个 hooks 文件里重复定义）。 */

/** 赛马排序指标。 */
export type RaceSortKey =
	| "totalTokens"
	| "requestCount"
	| "ttft"
	| "requestTime"
	| "tps"
	| "cacheHitRate";

/** 时间窗口（毫秒时间戳）。 */
export interface RaceWindow {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime: number;
}

export interface RaceSort {
	sortBy: RaceSortKey;
	sortOrder: "asc" | "desc";
}

/** 面板查询共享过滤维度（rank / charts / insight 各端点共用的子集）。 */
export interface StatsFilter {
	/** 按供应商过滤（可选）。 */
	providerId?: number;
	/** 按虚拟模型过滤（可选）。 */
	virtualModelId?: number;
	/** 按模型 ID 过滤（可选；供应商侧真实模型 ID）。 */
	modelId?: string;
	/** 按调用方 API Key 名称过滤（可选；API Key 数据面板用）。 */
	apiKey?: string;
}

/** 可选时间窗口（毫秒时间戳；缺省由后端回退默认窗口）。 */
export interface TimeWindowParams {
	startTime?: number;
	endTime?: number;
}
