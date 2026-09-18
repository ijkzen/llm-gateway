import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
	useNotificationChannel,
	useRegistrationStatus,
	useSaveNotificationChannel,
	useStartRegistration,
	useTestNotificationChannel,
} from "@/hooks/use-notification";
import { useToastActions } from "@/hooks/use-toast";
import type { ReceiverType } from "@/lib/notification";
import { QrCode, Send } from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

interface NotificationDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

interface FormState {
	appId: string;
	appSecret: string;
	receiverType: ReceiverType;
	receiver: string;
}

const EMPTY_FORM: FormState = {
	appId: "",
	appSecret: "",
	receiverType: "open_id",
	receiver: "",
};

/** 扫码状态 → 提示文案 key（success 不在此列，成功即回填表单）。 */
const QR_STATUS_KEYS: Record<string, string> = {
	pending: "notification.qrPending",
	slow_down: "notification.qrSlowDown",
	denied: "notification.qrDenied",
	expired: "notification.qrExpired",
};

/**
 * 飞书通知配置弹窗：创建（手填 App ID/Secret 或扫码一键创建）、修改、测试发送。
 * 凭据编辑时留空 App Secret 表示不修改（后端沿用库中原值）。
 */
export function NotificationDialog({ open, onOpenChange }: NotificationDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const { data: channel } = useNotificationChannel();
	const saveChannel = useSaveNotificationChannel();
	const testChannel = useTestNotificationChannel();
	const startRegistration = useStartRegistration();

	const [form, setForm] = useState<FormState>(EMPTY_FORM);
	const [enable, setEnable] = useState(true);
	const [editing, setEditing] = useState(false);
	const [sessionId, setSessionId] = useState<string | null>(null);
	const [qrUrl, setQrUrl] = useState("");

	const registration = useRegistrationStatus(sessionId);
	const configured = channel?.configured ?? false;
	// 配置未加载完不渲染表单：否则先渲染空表单、数据到达后被 effect 覆盖，
	// 用户在这几十毫秒内输入的内容会被静默清掉。
	const loaded = channel !== undefined;
	// 未配置时直接是填写态；已配置时默认只读，点「修改」进入编辑。
	const showForm = !configured || editing;

	useEffect(() => {
		if (!channel) return;
		setForm({
			appId: channel.appId,
			appSecret: "",
			receiverType: channel.receiverType,
			receiver: channel.receiver,
		});
		setEnable(channel.enable);
	}, [channel]);

	// 关闭弹窗时复位临时状态（下次打开回到只读/新建态）。
	useEffect(() => {
		if (open) return;
		setEditing(false);
		setSessionId(null);
		setQrUrl("");
	}, [open]);

	// 扫码成功 → 回填表单并切到编辑态，由用户确认后再保存。
	const registrationStatus = registration.data?.status;
	useEffect(() => {
		const data = registration.data;
		if (registrationStatus !== "success" || !data) return;
		setForm({
			appId: data.appId ?? "",
			appSecret: data.appSecret ?? "",
			receiverType: data.receiverType ?? "open_id",
			receiver: data.receiver ?? "",
		});
		setSessionId(null);
		setQrUrl("");
		setEditing(true);
		toastSuccess(t("notification.qrSuccess"));
	}, [registrationStatus, registration.data, t, toastSuccess]);

	const buildInput = () => ({
		appId: form.appId,
		// 留空 = 不修改（后端沿用库中原值）。
		appSecret: form.appSecret.trim() ? form.appSecret : undefined,
		receiverType: form.receiverType,
		receiver: form.receiver,
	});

	const handleSave = async () => {
		try {
			await saveChannel.mutateAsync({ ...buildInput(), enable });
			setForm((prev) => ({ ...prev, appSecret: "" }));
			setEditing(false);
			toastSuccess(t("common.success"));
		} catch (error) {
			toastError(t("notification.saveFailed"), error as Error);
		}
	};

	const handleTest = async () => {
		try {
			await testChannel.mutateAsync(buildInput());
			toastSuccess(t("notification.testSuccess"));
		} catch (error) {
			toastError(t("notification.testFailed"), error as Error);
		}
	};

	const handleStartRegistration = async () => {
		try {
			const session = await startRegistration.mutateAsync();
			setQrUrl(session.qrUrl);
			setSessionId(session.sessionId);
		} catch (error) {
			toastError(t("notification.qrFailed"), error as Error);
		}
	};

	const qrStatusKey = registrationStatus ? QR_STATUS_KEYS[registrationStatus] : undefined;

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="flex h-[min(680px,85vh)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[520px]">
				<DialogHeader className="shrink-0 space-y-3 px-6 pt-6 pb-4">
					<DialogTitle>{t("notification.title")}</DialogTitle>
					<DialogDescription>{t("notification.description")}</DialogDescription>
				</DialogHeader>

				{loaded && (
					<>
						<div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 pb-4">
							{configured && !editing && (
								<div className="space-y-3 rounded-lg border bg-card p-4 text-sm">
									<div className="flex items-center justify-between gap-3">
										<span className="text-muted-foreground">{t("notification.enableLabel")}</span>
										<Switch
											checked={enable}
											onCheckedChange={(next) => {
												setEnable(next);
												void saveChannel
													.mutateAsync({ ...buildInput(), enable: next })
													.catch((error) => {
														setEnable(!next);
														toastError(t("notification.saveFailed"), error as Error);
													});
											}}
											aria-label={t("notification.enableLabel")}
										/>
									</div>
									<div className="flex items-center justify-between gap-3">
										<span className="text-muted-foreground">App ID</span>
										<span className="font-mono">{channel?.appId}</span>
									</div>
									<div className="flex items-center justify-between gap-3">
										<span className="text-muted-foreground">App Secret</span>
										<span className="font-mono">{channel?.appSecretMasked}</span>
									</div>
									<div className="flex items-center justify-between gap-3">
										<span className="text-muted-foreground">{t("notification.receiverLabel")}</span>
										<span className="font-mono">{channel?.receiver}</span>
									</div>
									{channel?.lastError ? (
										<p className="text-destructive text-xs">
											{t("notification.lastError", { message: channel.lastError })}
										</p>
									) : channel?.lastSentAt ? (
										<p className="text-muted-foreground text-xs">
											{t("notification.lastSentAt", {
												time: new Date(channel.lastSentAt).toLocaleString(),
											})}
										</p>
									) : null}
								</div>
							)}

							{showForm && (
								<div className="space-y-3">
									<div className="space-y-1.5">
										<Label htmlFor="notification-app-id">App ID</Label>
										<Input
											id="notification-app-id"
											value={form.appId}
											onChange={(event) => setForm({ ...form, appId: event.target.value })}
											placeholder="cli_xxxxxxxx"
										/>
									</div>
									<div className="space-y-1.5">
										<Label htmlFor="notification-app-secret">App Secret</Label>
										<Input
											id="notification-app-secret"
											type="password"
											value={form.appSecret}
											onChange={(event) => setForm({ ...form, appSecret: event.target.value })}
											placeholder={configured ? t("notification.secretKeepHint") : ""}
										/>
									</div>
									<div className="space-y-1.5">
										<Label>{t("notification.receiverLabel")}</Label>
										<div className="flex gap-2">
											<Select
												value={form.receiverType}
												onValueChange={(value) =>
													setForm({ ...form, receiverType: value as ReceiverType })
												}
											>
												<SelectTrigger className="w-[130px] shrink-0">
													<SelectValue />
												</SelectTrigger>
												<SelectContent>
													<SelectItem value="open_id">
														{t("notification.receiverTypeOpenId")}
													</SelectItem>
													<SelectItem value="email">
														{t("notification.receiverTypeEmail")}
													</SelectItem>
												</SelectContent>
											</Select>
											<Input
												value={form.receiver}
												onChange={(event) => setForm({ ...form, receiver: event.target.value })}
												placeholder={t("notification.receiverPlaceholder")}
											/>
										</div>
									</div>
									<div className="flex items-center justify-between gap-3 rounded-lg border p-3">
										<span className="text-sm">{t("notification.enableLabel")}</span>
										<Switch
											checked={enable}
											onCheckedChange={setEnable}
											aria-label={t("notification.enableLabel")}
										/>
									</div>
								</div>
							)}

							<div className="space-y-3 rounded-lg border border-dashed p-4">
								<div className="flex items-center justify-between gap-3">
									<span className="flex items-center gap-2 text-sm font-medium">
										<QrCode className="size-4" />
										{t("notification.qrTitle")}
									</span>
									<Button
										variant="outline"
										size="sm"
										onClick={handleStartRegistration}
										disabled={startRegistration.isPending}
									>
										{startRegistration.isPending
											? t("common.loading")
											: qrUrl
												? t("notification.qrRefresh")
												: t("notification.qrStart")}
									</Button>
								</div>
								<p className="text-muted-foreground text-xs">{t("notification.qrHint")}</p>
								{qrUrl && (
									<div className="flex flex-col items-center gap-3">
										<div className="rounded-lg bg-white p-3">
											<QRCodeSVG value={qrUrl} size={168} data-testid="notification-qr" />
										</div>
										<p className="text-sm">
											{qrStatusKey ? t(qrStatusKey) : t("notification.qrWaiting")}
										</p>
									</div>
								)}
							</div>
						</div>

						<DialogFooter className="shrink-0 gap-2 border-t px-6 py-4 sm:justify-between">
							<Button variant="outline" onClick={handleTest} disabled={testChannel.isPending}>
								<Send className="size-4" />
								{t("notification.test")}
							</Button>
							<div className="flex gap-2">
								{configured && !editing ? (
									<Button onClick={() => setEditing(true)}>{t("common.edit")}</Button>
								) : (
									<>
										{configured && (
											<Button variant="outline" onClick={() => setEditing(false)}>
												{t("common.cancel")}
											</Button>
										)}
										<Button onClick={handleSave} disabled={saveChannel.isPending}>
											{t("common.save")}
										</Button>
									</>
								)}
							</div>
						</DialogFooter>
					</>
				)}
			</DialogContent>
		</Dialog>
	);
}
