import { ProviderEditDialog } from "@/components/providers/ProviderEditDialog";
import type { Provider, ProviderTemplate } from "@/hooks/use-providers";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		createMutate: vi.fn(),
		updateMutate: vi.fn(),
		toastSuccess: vi.fn(),
		toastError: vi.fn(),
		templates: [] as ProviderTemplate[],
	};
});

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useCreateProvider: () => ({ mutate: mocks.createMutate, isPending: false }),
		useUpdateProvider: () => ({ mutate: mocks.updateMutate, isPending: false }),
		useMatchTemplate: () => ({ data: mocks.templates }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: mocks.toastSuccess,
		toastError: mocks.toastError,
	}),
}));

function makeProvider(overrides: Partial<Provider> = {}): Provider {
	return {
		id: 9,
		name: "Test",
		enable: true,
		baseUrl: "https://token.sensenova.cn/v1",
		apiKeyMasked: "sk-****test",
		protocolType: 0,
		billingMode: 1,
		customHeader: "{}",
		extra: JSON.stringify({
			refresh_token: "rt-keep",
			username: "ijkzen",
			password: "secret-pw",
			usage: true,
			usage_type: 1,
		}),
		proxyEnabled: false,
		proxyAddr: "",
		createdAt: "2026-08-30T12:00:00Z",
		updatedAt: "2026-08-30T12:00:00Z",
		...overrides,
	};
}

beforeEach(() => {
	vi.clearAllMocks();
	mocks.templates = [];
});

describe("ProviderEditDialog：模板默认 custom_header 预填", () => {
	const kimiTemplate: ProviderTemplate = {
		name: "Kimi For Coding",
		baseUrl: "https://api.kimi.com/coding/v1",
		protocolType: 2,
		billingMode: 1,
		extra: "{}",
		customHeader: JSON.stringify({ "User-Agent": "KimiCLI/1.50.0" }),
	};

	it("创建模式应用模板：补填默认 custom_header，不覆盖已填键", () => {
		mocks.templates = [kimiTemplate];
		render(<ProviderEditDialog open onOpenChange={vi.fn()} provider={null} />);

		// 用户先手动填入一个自定义头键。
		fireEvent.click(screen.getByRole("button", { name: "高级设置" }));
		const headerInput = screen.getByLabelText("自定义请求头（JSON）");
		fireEvent.change(headerInput, { target: { value: '{"X-Keeper":"keep-me"}' } });

		// 应用模板：模板键补入，用户键保留。
		fireEvent.click(screen.getByRole("button", { name: "应用 Kimi For Coding 模板" }));
		const merged = JSON.parse(
			(screen.getByLabelText("自定义请求头（JSON）") as HTMLTextAreaElement).value,
		) as Record<string, string>;
		expect(merged).toMatchObject({
			"User-Agent": "KimiCLI/1.50.0",
			"X-Keeper": "keep-me",
		});
	});
});

describe("ProviderEditDialog：SenseNova 账号密码登录字段", () => {
	it("编辑时隐藏后端派生的 refresh_token，只展示 username/password（password 掩码）", () => {
		render(<ProviderEditDialog open onOpenChange={vi.fn()} provider={makeProvider()} />);
		// 展开高级设置（extra 字段区域）。
		fireEvent.click(screen.getByRole("button", { name: "高级设置" }));

		// 只显示 username/password，refresh_token 不渲染。
		expect(screen.getByLabelText("username")).toHaveValue("ijkzen");
		expect(screen.getByLabelText("password")).toHaveValue("secret-pw");
		expect(screen.getByLabelText("password")).toHaveAttribute("type", "password");
		expect(screen.queryByLabelText("refresh_token")).not.toBeInTheDocument();
	});

	it("保存时保留隐藏的 refresh_token（由后端维护）", async () => {
		render(<ProviderEditDialog open onOpenChange={vi.fn()} provider={makeProvider()} />);
		fireEvent.click(screen.getByRole("button", { name: "保存" }));

		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledTimes(1));
		const payload = mocks.updateMutate.mock.calls[0]?.[0] as {
			extra: string;
		};
		const extra = JSON.parse(payload.extra) as Record<string, unknown>;
		// refresh_token 保留（隐藏字段原样带出），usage/usage_type 标记保留。
		expect(extra.refresh_token).toBe("rt-keep");
		expect(extra.username).toBe("ijkzen");
		expect(extra.password).toBe("secret-pw");
		expect(extra.usage).toBe(true);
	});
});

describe("ProviderEditDialog Krill 凭据", () => {
	const krillProvider = makeProvider({
		id: 7,
		name: "Krill（按量付费）",
		baseUrl: "https://api-slb.krill-ai.net/v1",
		billingMode: 0,
		extra: JSON.stringify({
			email: "user@example.com",
			password: "secret",
			jwt: "jwt-existing",
			usage: true,
			usage_type: 0,
		}),
	});

	it("隐藏 JWT、密码使用密码框，并在保存时保留 JWT", async () => {
		render(<ProviderEditDialog open onOpenChange={vi.fn()} provider={krillProvider} />);
		fireEvent.click(screen.getByRole("button", { name: "高级设置" }));

		expect(screen.getByLabelText("email")).toHaveValue("user@example.com");
		expect(screen.getByLabelText("password")).toHaveAttribute("type", "password");
		expect(screen.queryByLabelText("jwt")).not.toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "保存" }));
		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledOnce());
		const payload = mocks.updateMutate.mock.calls[0]?.[0];
		expect(JSON.parse(payload.extra)).toMatchObject({
			email: "user@example.com",
			password: "secret",
			jwt: "jwt-existing",
			usage: true,
			usage_type: 0,
		});
	});
});
