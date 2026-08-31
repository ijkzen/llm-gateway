import i18n from "@/i18n";
import ky, { type AfterResponseHook, type BeforeErrorHook, type HTTPError } from "ky";

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
		public readonly statusCode?: number,
	) {
		super(message);
		this.name = "ApiError";
	}
}

const beforeErrorHook: BeforeErrorHook = async (error) => {
	const { response } = error;
	if (response) {
		try {
			const body = (await response.clone().json()) as unknown;
			if (isApiResponse(body)) {
				return new ApiError(
					body.msg || error.message,
					body.code || `HTTP_${response.status}`,
					response.status,
				) as unknown as HTTPError;
			}
		} catch {
			// 不是合法 JSON 或不符合 ApiResponse 结构
		}
		return new ApiError(
			error.message,
			`HTTP_${response.status}`,
			response.status,
		) as unknown as HTTPError;
	}
	return new ApiError(error.message, "NETWORK_ERROR") as unknown as HTTPError;
};

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
			window.location.assign("/login");
		}
	}
	return response;
};

export const api = ky.create({
	prefixUrl: "/api",
	timeout: 10000,
	retry: 1,
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
