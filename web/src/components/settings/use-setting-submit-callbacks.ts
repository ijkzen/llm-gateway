import { useToastActions } from "@/hooks/use-toast";

/**
 * 设置域弹窗共用的提交回调（18-17）：成功关窗 + 成功提示，失败提示。
 * 三处调用方（设置值编辑 / JSON 编辑 / 删除）此前各自展开同一段样板，
 * 载荷类型各异故只收敛回调、由调用方继续写 `mutation.mutate(payload, callbacks)`。
 */
export function useSettingSubmitCallbacks(
	onOpenChange: (open: boolean) => void,
	successMessage: string,
	failureMessage: string,
) {
	const { toastSuccess, toastError } = useToastActions();
	return {
		onSuccess: () => {
			onOpenChange(false);
			toastSuccess(successMessage);
		},
		onError: (error: unknown) => toastError(failureMessage, error),
	};
}
