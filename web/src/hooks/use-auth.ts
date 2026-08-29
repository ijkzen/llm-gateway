import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

export interface AuthStatus {
	initialized: boolean;
}

export interface AuthUser {
	username: string;
}

export const authKeys = {
	status: ["auth", "status"] as const,
	me: ["auth", "me"] as const,
};

export function useAuthStatus() {
	return useQuery<AuthStatus>({
		queryKey: authKeys.status,
		queryFn: async () => {
			const res = await api.get("auth/status").json<ApiResponse<AuthStatus>>();
			return unwrap(res);
		},
		staleTime: Number.POSITIVE_INFINITY,
		retry: false,
	});
}

export function useMe() {
	return useQuery<AuthUser>({
		queryKey: authKeys.me,
		queryFn: async () => {
			const res = await api.get("auth/me").json<ApiResponse<AuthUser>>();
			return unwrap(res);
		},
		retry: false,
		staleTime: 5 * 60 * 1000,
	});
}

function useAuthAction(endpoint: "login" | "init") {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async ({ username, password }: { username: string; password: string }) => {
			const res = await api
				.post(`auth/${endpoint}`, { json: { username, password } })
				.json<ApiResponse<AuthUser>>();
			return unwrap(res);
		},
		onSuccess: (user) => {
			queryClient.setQueryData(authKeys.me, user);
			queryClient.setQueryData(authKeys.status, { initialized: true } satisfies AuthStatus);
		},
	});
}

export function useLogin() {
	return useAuthAction("login");
}

export function useInitAdmin() {
	return useAuthAction("init");
}

export function useLogout() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async () => {
			const res = await api.post("auth/logout").json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSettled: () => {
			queryClient.setQueryData(authKeys.me, null);
		},
	});
}

export function useChangePassword() {
	return useMutation({
		mutationFn: async ({
			oldPassword,
			newPassword,
		}: { oldPassword: string; newPassword: string }) => {
			const res = await api
				.post("auth/change-password", { json: { oldPassword, newPassword } })
				.json<ApiResponse<unknown>>();
			return unwrap(res);
		},
	});
}
