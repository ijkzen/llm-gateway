import { ProviderEditDialog } from "@/components/providers/ProviderEditDialog";
import type { Provider } from "@/hooks/use-providers";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		createMutate: vi.fn(),
		updateMutate: vi.fn(),
		toastSuccess: vi.fn(),
		toastError: vi.fn(),
	};
});

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useCreateProvider: () => ({ mutate: mocks.createMutate, isPending: false }),
		useUpdateProvider: () => ({ mutate: mocks.updateMutate, isPending: false }),
		useMatchTemplate: () => ({ data: [] }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: mocks.toastSuccess,
		toastError: mocks.toastError,
	}),
}));

/** SenseNova 编辑场景：usage 开启、含后端派生 refresh_token 与用户维护的 username/password。 */
function makeSensenovaProvider(): Provider {
	return {
		id: 9,
		name: "SenseNova",
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
	};
}

beforeEach(() => {
	vi.clearAllMocks();
});

describe("ProviderEditDialog：SenseNova 账号密码登录字段", () => {
	it("编辑时隐藏后端派生的 refresh_token，只展示 username/password（password 掩码）", () => {
		render(<ProviderEditDialog open onOpenChange={vi.fn()} provider={makeSensenovaProvider()} />);
		// 展开高级设置（extra 字段区域）。
		fireEvent.click(screen.getByRole("button", { name: "高级设置" }));

		// 只显示 username/password，refresh_token 不渲染。
		expect(screen.getByLabelText("username")).toHaveValue("ijkzen");
		expect(screen.getByLabelText("password")).toHaveValue("secret-pw");
		expect(screen.getByLabelText("password")).toHaveAttribute("type", "password");
		expect(screen.queryByLabelText("refresh_token")).not.toBeInTheDocument();
	});

	it("保存时保留隐藏的 refresh_token（由后端维护）", async () => {
		render(<ProviderEditDialog open onOpenChange={vi.fn()} provider={makeSensenovaProvider()} />);
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
