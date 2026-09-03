import { ProviderEditDialog } from "@/components/providers/ProviderEditDialog";
import type { Provider } from "@/hooks/use-providers";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	updateMutate: vi.fn(),
}));

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useCreateProvider: () => ({ mutate: vi.fn(), isPending: false }),
		useUpdateProvider: () => ({ mutate: mocks.updateMutate, isPending: false }),
		useMatchTemplate: () => ({ data: [] }),
	};
});

const provider: Provider = {
	id: 7,
	name: "Krill（按量付费）",
	enable: true,
	baseUrl: "https://api-slb.krill-ai.net/v1",
	apiKeyMasked: "sk-****test",
	protocolType: 0,
	billingMode: 0,
	customHeader: "{}",
	extra: JSON.stringify({
		email: "user@example.com",
		password: "secret",
		jwt: "jwt-existing",
		usage: true,
		usage_type: 0,
	}),
	proxyEnabled: false,
	proxyAddr: "",
	createdAt: "2026-09-03T00:00:00Z",
	updatedAt: "2026-09-03T00:00:00Z",
};

describe("ProviderEditDialog Krill 凭据", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("隐藏 JWT、密码使用密码框，并在保存时保留 JWT", async () => {
		render(<ProviderEditDialog open onOpenChange={vi.fn()} provider={provider} />);
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
