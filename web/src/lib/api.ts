import i18n from "@/i18n";
import ky, { type AfterResponseHook, type BeforeErrorHook, HTTPError, TimeoutError } from "ky";

export interface ApiResponse<T> {
	code: string;
	msg: string;
	data?: T;
}

function isApiResponse(body: unknown): body is ApiResponse<unknown> {
	return (
		typeof body === "object" &&
		body !== null &&
		"code" in body &&
		"msg" in body &&
		typeof (body as Record<string, unknown>).code === "string" &&
		typeof (body as Record<string, unknown>).msg === "string"
	);
}

export class ApiError extends Error {
	constructor(
		message: string,
		public readonly code: string,
	) {
		super(message);
		this.name = "ApiError";
	}
}

type HttpErrorWithCode = HTTPError & { apiCode?: string };

/** 读取后端错误信封里的业务码（beforeError 已解析缓存），无则回退 HTTP_<status>。 */
function apiCodeOf(error: HTTPError): string {
	const cached = (error as HttpErrorWithCode).apiCode;
	if (cached) return cached;
	return `HTTP_${error.response.status}`;
}

/**
 * beforeError：把后端信封的 msg/code 合并到原 HTTPError 上。
 *
 * 不替换错误身份（19-02）：ky 的重试决策靠 `isHTTPError()` 判定，返回
 * `new ApiError(...)` 会让 `retry.statusCodes` 白名单与 Retry-After 尊重
 * 全部失效（确定性 4xx 也被重试）。只改 message 并挂业务码字段。
 */
export const beforeErrorHook: BeforeErrorHook = async (error) => {
	const { response } = error;
	try {
		const body = (await response.clone().json()) as unknown;
		if (isApiResponse(body) && body.msg) {
			error.message = body.msg;
			(error as HttpErrorWithCode).apiCode = body.code || apiCodeOf(error);
			return error;
		}
	} catch {
		// 不是合法 JSON 或不符合 ApiResponse 结构：保留 ky 原始 message。
	}
	(error as HttpErrorWithCode).apiCode = apiCodeOf(error);
	return error;
};

/** 会话过期跳转前暂存来源路径的 sessionStorage 键（19-14：登录后回跳）。 */
export const AUTH_REDIRECT_FROM_KEY = "lg.auth.from";

/**
 * 全局 401 处理：会话过期时跳转登录页。
 * 认证接口本身（/api/auth/*）与登录页内的请求不触发跳转，避免死循环。
 */
const afterResponseHook: AfterResponseHook = async (request, _options, response) => {
	if (response.status === 401 && typeof window !== "undefined") {
		const url = new URL(request.url);
		const isAuthEndpoint = url.pathname.startsWith("/api/auth/");
		const onLoginPage = window.location.pathname.startsWith("/login");
		if (!isAuthEndpoint && !onLoginPage) {
			// 19-14：整页跳转会丢失 react-router 的 state.from，先落到 sessionStorage，
			// 登录页从中恢复——会话过期后仍能回到原页面。
			try {
				window.sessionStorage.setItem(
					AUTH_REDIRECT_FROM_KEY,
					`${window.location.pathname}${window.location.search}`,
				);
			} catch {
				// 隐私模式等禁用 storage：退化为不回跳，不影响跳转本身。
			}
			window.location.assign("/login");
		}
	}
	return response;
};

/** 读取并清除会话过期时暂存的来源路径（19-14；读取即消费，避免长期残留）。 */
export function readStoredRedirectFrom(): string | null {
	if (typeof window === "undefined") return null;
	try {
		const value = window.sessionStorage.getItem(AUTH_REDIRECT_FROM_KEY);
		if (value) window.sessionStorage.removeItem(AUTH_REDIRECT_FROM_KEY);
		return value;
	} catch {
		return null;
	}
}

export const api = ky.create({
	prefixUrl: "/api",
	timeout: 30000,
	// 19-13：HTTP 层不自带重试——重试统一由 react-query 的 `retry: 1` 承担
	// （两层各一次会让单次失败打满 4 次请求）。
	retry: 0,
	hooks: {
		afterResponse: [afterResponseHook],
		beforeError: [beforeErrorHook],
	},
});

export interface HealthInfo {
	status: string;
	version?: string;
}

/** 探活并读取服务版本（healthz 是健康检查接口，不走 ApiResponse 信封）。 */
export async function fetchHealth(): Promise<HealthInfo> {
	return (await api.get("healthz").json()) as HealthInfo;
}

export async function unwrap<T>(res: ApiResponse<T>): Promise<T> {
	if (res.code !== "0") {
		throw new ApiError(res.msg || i18n.t("common.error"), res.code);
	}
	if (res.data === undefined) {
		throw new ApiError(i18n.t("error.missingData"), "MISSING_DATA");
	}
	return res.data;
}

/** 业务码取值（兼容 ApiError 与 HTTPError；其他错误返回 undefined）。 */
export function apiCodeOfError(error: unknown): string | undefined {
	if (error instanceof ApiError) return error.code;
	if (error instanceof HTTPError) return apiCodeOf(error);
	return undefined;
}

/**
 * 用户可见错误消息：网络/超时错误不过 ky 的 beforeError（19-03），在此统一
 * 映射为本地化文案；HTTPError 的 message 已被 beforeError 换成后端中文 msg。
 */
export function userErrorMessage(error: unknown): string {
	if (error instanceof DOMException && error.name === "AbortError") {
		return i18n.t("error.aborted");
	}
	if (error instanceof TimeoutError) {
		const method = error.request?.method ?? "";
		const raw = error.request?.url ?? "";
		const url = raw.startsWith(window.location.origin)
			? raw.slice(window.location.origin.length)
			: (() => {
					try {
						const parsed = new URL(raw);
						return parsed.pathname + parsed.search;
					} catch {
						return raw;
					}
				})();
		return i18n.t("error.timeout", { method, url });
	}
	if (error instanceof HTTPError || error instanceof ApiError) {
		return error.message || i18n.t("common.error");
	}
	if (error instanceof TypeError) {
		return i18n.t("error.networkError");
	}
	if (error instanceof Error && error.message) {
		return error.message;
	}
	return i18n.t("common.error");
}
