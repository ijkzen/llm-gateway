import { useMemo } from "react";
import { toast } from "sonner";

// 基于 sonner 的轻量封装，保持业务代码原有的调用方式不变
export function useToastActions() {
	return useMemo(
		() => ({
			toastSuccess: (title: string) => toast.success(title),
			toastError: (title: string, error: Error) =>
				toast.error(title, { description: error.message }),
		}),
		[],
	);
}
