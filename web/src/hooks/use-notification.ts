import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
	type FeishuConfigInput,
	fetchNotificationChannel,
	fetchRegistrationStatus,
	saveNotificationChannel,
	startRegistration,
	testNotificationChannel,
} from "@/lib/notification";

export const notificationKeys = {
	all: ["notification"] as const,
	registration: (sessionId: string) => ["notification", "registration", sessionId] as const,
};

/** 读取飞书通知渠道配置（未配置时返回 configured:false 的空壳）。 */
export function useNotificationChannel() {
	return useQuery({
		queryKey: notificationKeys.all,
		queryFn: fetchNotificationChannel,
	});
}

/** 保存配置（appSecret 缺省时不覆盖库中原值）。 */
export function useSaveNotificationChannel() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: (input: FeishuConfigInput & { enable: boolean }) => saveNotificationChannel(input),
		onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.all }),
	});
}

/** 用表单当前值发一条测试消息。 */
export function useTestNotificationChannel() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: (input: FeishuConfigInput) => testNotificationChannel(input),
		// 发送结果会回写 lastError / lastSentAt，重取配置让面板同步。
		onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.all }),
	});
}

/** 发起扫码创建会话。 */
export function useStartRegistration() {
	return useMutation({ mutationFn: startRegistration });
}

/** 轮询扫码状态：等待中/需退避时每 2 秒拉一次，拿到终态即停。 */
export function useRegistrationStatus(sessionId: string | null) {
	return useQuery({
		queryKey: notificationKeys.registration(sessionId ?? ""),
		queryFn: () => fetchRegistrationStatus(sessionId ?? ""),
		enabled: !!sessionId,
		refetchInterval: (query) => {
			const status = query.state.data?.status;
			return status === "pending" || status === "slow_down" ? 2000 : false;
		},
	});
}
