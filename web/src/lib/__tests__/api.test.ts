import {
	AUTH_REDIRECT_FROM_KEY,
	apiCodeOfError,
	beforeErrorHook,
	readStoredRedirectFrom,
	unwrap,
	userErrorMessage,
} from "@/lib/api";
import { HTTPError, TimeoutError } from "ky";
import { afterEach, describe, expect, it } from "vitest";

function httpError(status: number, body?: unknown): HTTPError {
	const response = new Response(body === undefined ? null : JSON.stringify(body), {
		status,
		statusText: "Error",
		headers: { "content-type": "application/json" },
	});
	const request = new Request("http://localhost/api/x", { method: "GET" });
	return new HTTPError(response, request, {} as never);
}

describe("api beforeError", () => {
	it("后端信封的 msg/code 合入 HTTPError（不换错误身份，19-02）", async () => {
		const error = httpError(400, { code: "BAD_REQUEST", msg: "参数非法" });
		const mapped = await beforeErrorHook(error, {} as never);
		expect(mapped).toBe(error);
		expect(mapped?.message).toBe("参数非法");
		expect(apiCodeOfError(mapped)).toBe("BAD_REQUEST");
	});

	it("非信封体保留原始 message 并回退 HTTP_<status> 码", async () => {
		const error = httpError(500, { detail: "boom" });
		const mapped = await beforeErrorHook(error, {} as never);
		expect(mapped.message).toContain("500");
		expect(apiCodeOfError(mapped)).toBe("HTTP_500");
	});
});

describe("userErrorMessage", () => {
	it("网络类 TypeError 映射为本地化文案", () => {
		expect(userErrorMessage(new TypeError("Failed to fetch"))).toBe("网络请求失败");
	});

	it("超时错误带方法与地址", () => {
		const error = new TimeoutError(new Request("http://localhost/api/slow", { method: "GET" }));
		expect(userErrorMessage(error)).toBe("请求超时：GET /api/slow");
	});

	it("HTTPError 直接用后端 msg", () => {
		const error = httpError(503, { code: "UPSTREAM_ERROR", msg: "上游不可用" });
		error.message = "上游不可用";
		expect(userErrorMessage(error)).toBe("上游不可用");
	});

	it("普通 Error 原样返回", () => {
		expect(userErrorMessage(new Error("普通错误"))).toBe("普通错误");
	});
});

describe("unwrap", () => {
	it("code 非 0 抛 ApiError 且带业务码", async () => {
		await expect(unwrap({ code: "X", msg: "出错了" })).rejects.toMatchObject({
			name: "ApiError",
			code: "X",
			message: "出错了",
		});
	});

	it("缺 data 抛 MISSING_DATA", async () => {
		await expect(unwrap({ code: "0", msg: "ok" })).rejects.toMatchObject({
			code: "MISSING_DATA",
		});
	});
});

describe("401 跳转的来源路径暂存（19-14）", () => {
	afterEach(() => {
		window.sessionStorage.clear();
	});

	it("读取即消费：首次读出后即清除", () => {
		window.sessionStorage.setItem(AUTH_REDIRECT_FROM_KEY, "/providers?x=1");
		expect(readStoredRedirectFrom()).toBe("/providers?x=1");
		expect(readStoredRedirectFrom()).toBeNull();
	});

	it("无暂存时返回 null", () => {
		expect(readStoredRedirectFrom()).toBeNull();
	});
});
