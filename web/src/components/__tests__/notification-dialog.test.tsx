import { NotificationDialog } from "@/components/settings/NotificationDialog";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	fetchChannel: vi.fn(),
	saveChannel: vi.fn(),
	testChannel: vi.fn(),
	startRegistration: vi.fn(),
	fetchStatus: vi.fn(),
	toastSuccess: vi.fn(),
	toastError: vi.fn(),
}));

vi.mock("@/lib/notification", () => ({
	fetchNotificationChannel: mocks.fetchChannel,
	saveNotificationChannel: mocks.saveChannel,
	testNotificationChannel: mocks.testChannel,
	startRegistration: mocks.startRegistration,
	fetchRegistrationStatus: mocks.fetchStatus,
}));

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({ toastSuccess: mocks.toastSuccess, toastError: mocks.toastError }),
}));

function renderDialog() {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>
			<NotificationDialog open onOpenChange={vi.fn()} />
		</QueryClientProvider>,
	);
}

const UNCONFIGURED = {
	configured: false,
	appId: "",
	appSecretMasked: "",
	receiverType: "open_id" as const,
	receiver: "",
	enable: true,
	lastError: "",
	lastSentAt: null,
};

const CONFIGURED = {
	configured: true,
	appId: "cli_existing",
	appSecretMasked: "cli****1234",
	receiverType: "open_id" as const,
	receiver: "ou_owner",
	enable: true,
	lastError: "",
	lastSentAt: "2026-09-18T03:30:00Z",
};

/** 填好手填表单（未配置态）。 */
function fillForm({
	appId,
	secret,
	receiver,
}: { appId: string; secret: string; receiver: string }) {
	fireEvent.change(screen.getByLabelText("App ID"), { target: { value: appId } });
	fireEvent.change(screen.getByLabelText("App Secret"), { target: { value: secret } });
	fireEvent.change(screen.getByPlaceholderText("ou_xxx 或 ops@example.com"), {
		target: { value: receiver },
	});
}

describe("NotificationDialog 未配置态", () => {
	beforeEach(() => {
		vi.restoreAllMocks();
		mocks.fetchChannel.mockReset().mockResolvedValue(UNCONFIGURED);
		mocks.saveChannel.mockReset().mockResolvedValue(CONFIGURED);
		mocks.testChannel.mockReset().mockResolvedValue(undefined);
		mocks.toastSuccess.mockReset();
		mocks.toastError.mockReset();
	});

	it("同时展示手动填写与扫码创建两条路径", async () => {
		renderDialog();
		expect(await screen.findByLabelText("App ID")).toBeInTheDocument();
		expect(screen.getByLabelText("App Secret")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: /扫码创建/ })).toBeInTheDocument();
	});

	it("手动保存提交完整配置（含 appSecret 与接收者）", async () => {
		renderDialog();
		await screen.findByLabelText("App ID");
		fillForm({ appId: "cli_new", secret: "secret-1234567", receiver: "ou_me" });

		fireEvent.click(screen.getByRole("button", { name: "保存" }));

		await waitFor(() =>
			expect(mocks.saveChannel).toHaveBeenCalledWith({
				appId: "cli_new",
				appSecret: "secret-1234567",
				receiverType: "open_id",
				receiver: "ou_me",
				enable: true,
			}),
		);
		expect(mocks.toastSuccess).toHaveBeenCalled();
	});

	it("测试发送用表单当前值（未保存也能测）", async () => {
		renderDialog();
		await screen.findByLabelText("App ID");
		fillForm({ appId: "cli_new", secret: "secret-1234567", receiver: "ou_me" });

		fireEvent.click(screen.getByRole("button", { name: /测试发送/ }));

		await waitFor(() =>
			expect(mocks.testChannel).toHaveBeenCalledWith({
				appId: "cli_new",
				appSecret: "secret-1234567",
				receiverType: "open_id",
				receiver: "ou_me",
			}),
		);
		expect(mocks.toastSuccess).toHaveBeenCalled();
	});

	it("测试发送失败时展示后端返回的中文原因", async () => {
		mocks.testChannel.mockRejectedValue(
			new Error("飞书应用缺少发送消息权限（im:message:send_as_bot）"),
		);
		renderDialog();
		await screen.findByLabelText("App ID");
		fillForm({ appId: "cli_new", secret: "secret-1234567", receiver: "ou_me" });

		fireEvent.click(screen.getByRole("button", { name: /测试发送/ }));

		await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
		const error = mocks.toastError.mock.calls[0]?.[1] as Error;
		expect(error.message).toContain("权限");
	});
});

describe("NotificationDialog 已配置态", () => {
	beforeEach(() => {
		vi.restoreAllMocks();
		mocks.fetchChannel.mockReset().mockResolvedValue(CONFIGURED);
		mocks.saveChannel.mockReset().mockResolvedValue(CONFIGURED);
		mocks.testChannel.mockReset().mockResolvedValue(undefined);
		mocks.toastSuccess.mockReset();
		mocks.toastError.mockReset();
	});

	it("展示掩码后的 App Secret，不回显明文", async () => {
		renderDialog();
		expect(await screen.findByText("cli_existing")).toBeInTheDocument();
		expect(screen.getByText("cli****1234")).toBeInTheDocument();
		expect(screen.getByText("ou_owner")).toBeInTheDocument();
		// 只读态不渲染输入框。
		expect(screen.queryByLabelText("App Secret")).toBeNull();
	});

	it("修改时 App Secret 留空提交 → 请求体不带 appSecret", async () => {
		renderDialog();
		await screen.findByText("cli_existing");
		fireEvent.click(screen.getByRole("button", { name: "编辑" }));

		// 只改接收者，Secret 留空。
		fireEvent.change(screen.getByPlaceholderText("ou_xxx 或 ops@example.com"), {
			target: { value: "ou_other" },
		});
		fireEvent.click(screen.getByRole("button", { name: "保存" }));

		await waitFor(() => expect(mocks.saveChannel).toHaveBeenCalled());
		const payload = mocks.saveChannel.mock.calls[0]?.[0] as Record<string, unknown>;
		expect(payload.receiver).toBe("ou_other");
		expect(payload.appSecret).toBeUndefined();
	});

	it("展示最近一次发送结果（失败原因）", async () => {
		mocks.fetchChannel.mockResolvedValue({
			...CONFIGURED,
			lastError: "接收者不在机器人的可用范围内",
		});
		renderDialog();
		expect(await screen.findByText(/接收者不在机器人的可用范围内/)).toBeInTheDocument();
	});
});

describe("NotificationDialog 扫码创建", () => {
	beforeEach(() => {
		vi.restoreAllMocks();
		mocks.fetchChannel.mockReset().mockResolvedValue(UNCONFIGURED);
		mocks.saveChannel.mockReset().mockResolvedValue(CONFIGURED);
		mocks.testChannel.mockReset().mockResolvedValue(undefined);
		mocks.startRegistration.mockReset().mockResolvedValue({
			sessionId: "s-1",
			qrUrl: "https://open.feishu.cn/page/launcher?user_code=AB",
			expiresIn: 600,
		});
		mocks.fetchStatus.mockReset().mockResolvedValue({ status: "pending" });
		mocks.toastSuccess.mockReset();
		mocks.toastError.mockReset();
	});

	it("点扫码创建渲染二维码并显示等待文案", async () => {
		renderDialog();
		await screen.findByLabelText("App ID");

		fireEvent.click(screen.getByRole("button", { name: /扫码创建/ }));

		await waitFor(() => expect(mocks.startRegistration).toHaveBeenCalled());
		await waitFor(() => expect(screen.getByTestId("notification-qr")).toBeInTheDocument());
		await waitFor(() => expect(mocks.fetchStatus).toHaveBeenCalledWith("s-1"));
	});

	it("扫码成功后回填表单（不自动保存）", async () => {
		mocks.fetchStatus.mockResolvedValue({
			status: "success",
			appId: "cli_scanned",
			appSecret: "scanned-secret-7777",
			receiver: "ou_scanner",
			receiverType: "open_id",
		});
		renderDialog();
		await screen.findByLabelText("App ID");

		fireEvent.click(screen.getByRole("button", { name: /扫码创建/ }));

		await waitFor(() =>
			expect((screen.getByLabelText("App ID") as HTMLInputElement).value).toBe("cli_scanned"),
		);
		expect((screen.getByLabelText("App Secret") as HTMLInputElement).value).toBe(
			"scanned-secret-7777",
		);
		expect(
			(screen.getByPlaceholderText("ou_xxx 或 ops@example.com") as HTMLInputElement).value,
		).toBe("ou_scanner");
		// 只回填，不落库——保存必须由用户点。
		expect(mocks.saveChannel).not.toHaveBeenCalled();
	});

	it("过期态提示重新获取", async () => {
		mocks.fetchStatus.mockResolvedValue({ status: "expired" });
		renderDialog();
		await screen.findByLabelText("App ID");

		fireEvent.click(screen.getByRole("button", { name: /扫码创建/ }));

		await waitFor(() => expect(screen.getByText(/二维码已过期/)).toBeInTheDocument());
		expect(screen.getByRole("button", { name: /重新获取/ })).toBeInTheDocument();
	});
});
