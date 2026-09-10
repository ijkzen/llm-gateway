import { userErrorMessage } from "@/lib/api";
import { useMemo } from "react";
import { toast } from "sonner";

// 基于 sonner 的轻量封装，保持业务代码原有的调用方式不变
export function useToastActions() {
	return useMemo(
		() => ({
			toastSuccess: (title: string) => toast.success(title),
			// 错误描述统一经 userErrorMessage：网络/超时错误不过 ky 的 beforeError，
			// 直接取 message 会显示英文原文（19-03）。
			toastError: (title: string, error: unknown) =>
				toast.error(title, { description: userErrorMessage(error) }),
		}),
		[],
	);
}
