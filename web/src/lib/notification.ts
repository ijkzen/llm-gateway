import { type ApiResponse, api, unwrap } from "@/lib/api";

/** 接收者类型：飞书 receive_id_type 的取值子集。 */
export type ReceiverType = "open_id" | "email";

/** 通知渠道配置（appSecret 由后端掩码，前端不回显明文）。 */
export interface NotificationChannel {
	configured: boolean;
	appId: string;
	appSecretMasked: string;
	receiverType: ReceiverType;
	receiver: string;
	enable: boolean;
	lastError: string;
	lastSentAt: string | null;
}

/** 保存/测试发送的入参；`appSecret` 缺省表示沿用库中原凭据。 */
export interface FeishuConfigInput {
	appId: string;
	appSecret?: string;
	receiverType: ReceiverType;
	receiver: string;
}

const BASE = "notification/feishu";

export async function fetchNotificationChannel(): Promise<NotificationChannel> {
	const res = await api.get(BASE).json<ApiResponse<NotificationChannel>>();
	return unwrap(res);
}

/** 保存配置（appSecret 缺省时不覆盖库中原值）。 */
export async function saveNotificationChannel(
	input: FeishuConfigInput & { enable: boolean },
): Promise<NotificationChannel> {
	const res = await api.put(BASE, { json: input }).json<ApiResponse<NotificationChannel>>();
	return unwrap(res);
}

/** 用当前表单值真实发一条测试消息（不落库配置）。 */
export async function testNotificationChannel(input: FeishuConfigInput): Promise<void> {
	const res = await api.post(`${BASE}/test`, { json: input }).json<ApiResponse<unknown>>();
	unwrap(res);
}

export interface RegistrationSession {
	sessionId: string;
	qrUrl: string;
	expiresIn: number;
}

export type RegistrationStatus = "pending" | "slow_down" | "success" | "denied" | "expired";

export interface RegistrationState {
	status: RegistrationStatus;
	appId?: string;
	appSecret?: string;
	receiver?: string;
	receiverType?: ReceiverType;
}

/** 发起扫码创建会话，拿到二维码内容。 */
export async function startRegistration(): Promise<RegistrationSession> {
	const res = await api.post(`${BASE}/register`).json<ApiResponse<RegistrationSession>>();
	return unwrap(res);
}

/** 轮询扫码状态（终态带回凭据）。 */
export async function fetchRegistrationStatus(sessionId: string): Promise<RegistrationState> {
	const res = await api.get(`${BASE}/register/${sessionId}`).json<ApiResponse<RegistrationState>>();
	return unwrap(res);
}
