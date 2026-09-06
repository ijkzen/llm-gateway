import { describe, expect, it } from "vitest";

import { statsFilterKeySegments, statsKey, statsSearchParams } from "@/hooks/stats-query";

describe("statsSearchParams", () => {
	it("跳过 undefined，数字转字符串", () => {
		expect(
			statsSearchParams({
				sortBy: "totalTokens",
				startTime: 100,
				apiKey: undefined,
				providerId: 7,
			}),
		).toBe("sortBy=totalTokens&startTime=100&providerId=7");
	});

	it("全 undefined 返回空串", () => {
		expect(statsSearchParams({ startTime: undefined })).toBe("");
	});

	it("空串与 0 是有效值不被跳过", () => {
		expect(statsSearchParams({ modelId: "", endTime: 0 })).toBe("modelId=&endTime=0");
	});
});

describe("statsKey", () => {
	it("前缀 stats + 端点名，undefined 归一为 null", () => {
		expect(statsKey("provider-rank", [1, "desc", undefined, null])).toEqual([
			"stats",
			"provider-rank",
			1,
			"desc",
			null,
			null,
		]);
	});
});

describe("statsFilterKeySegments", () => {
	it("固定顺序 providerId/virtualModelId/modelId/apiKey", () => {
		expect(statsFilterKeySegments({ modelId: "glm-4.5", apiKey: "itest" })).toEqual([
			null,
			null,
			"glm-4.5",
			"itest",
		]);
	});
});
