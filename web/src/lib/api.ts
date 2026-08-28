import ky, { type BeforeErrorHook, type HTTPError } from "ky";

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

export const api = ky.create({
	prefixUrl: "/api",
	timeout: 10000,
	retry: 1,
	hooks: {
		beforeError: [beforeErrorHook],
	},
});

export async function unwrap<T>(res: ApiResponse<T>): Promise<T> {
	if (res.code !== "0") {
		throw new ApiError(res.msg || "请求失败", res.code);
	}
	if (res.data === undefined) {
		throw new ApiError("响应中缺少 data 字段", "MISSING_DATA");
	}
	return res.data;
}
