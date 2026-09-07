import { BackupDialog } from "@/components/settings/BackupDialog";
import { ImportDialog } from "@/components/settings/ImportDialog";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	exportBackup: vi.fn(),
	importBackup: vi.fn(),
	toastSuccess: vi.fn(),
	toastError: vi.fn(),
	invalidate: vi.fn(),
	anchorClick: vi.fn(),
}));

vi.mock("@/lib/backup", () => ({
	fetchBackupExport: mocks.exportBackup,
	importBackup: mocks.importBackup,
}));

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({ toastSuccess: mocks.toastSuccess, toastError: mocks.toastError }),
}));

vi.mock("@tanstack/react-query", async () => {
	const actual =
		await vi.importActual<typeof import("@tanstack/react-query")>("@tanstack/react-query");
	return {
		...actual,
		useQueryClient: () => ({ invalidateQueries: mocks.invalidate }),
	};
});

/** BackupDialog 受控包装：点「恢复备份」时真实关闭备份弹窗、打开导入弹窗。 */
function BackupHarness() {
	const [open, setOpen] = useState(true);
	return <BackupDialog open={open} onOpenChange={setOpen} />;
}

function renderWithProvider(ui: React.ReactElement) {
	const queryClient = new QueryClient();
	return render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>);
}

/** 补充 jsdom 缺失的 URL API 并拦截 <a> 下载点击。 */
function stubDownload() {
	Object.defineProperty(URL, "createObjectURL", {
		value: vi.fn(() => "blob:mock"),
		configurable: true,
	});
	Object.defineProperty(URL, "revokeObjectURL", {
		value: vi.fn(() => {}),
		configurable: true,
	});
	const origCreate = document.createElement.bind(document);
	vi.spyOn(document, "createElement").mockImplementation((tag: string) => {
		const el = origCreate(tag);
		if (tag === "a") {
			el.click = () => mocks.anchorClick();
		}
		return el;
	});
}

/** 构造 .json File 并触发 input change；返回一个 Promise 等待 FileReader 完成。 */
async function attachFile(content: string) {
	const input = document.querySelector('input[type="file"]') as HTMLInputElement;
	const file = new File([content], "backup.json", { type: "application/json" });
	fireEvent.change(input, { target: { files: [file] } });
	await waitFor(() => expect(screen.getByText(/backup\.json/)).toBeInTheDocument());
}

const MINIMAL_JSON = JSON.stringify({
	version: 1,
	providers: [],
	virtualModels: [],
	apiKeys: [],
	settings: [],
});

/** 返回所有「确认导入」按钮中的最后一个（破坏性确认里的 AlertDialogAction，portal 后挂载）。 */
function clickConfirmDialogAction() {
	const actions = screen.getAllByRole("button", { name: "确认导入" });
	const action = actions[actions.length - 1];
	if (action) fireEvent.click(action);
}

describe("BackupDialog", () => {
	beforeEach(() => {
		vi.restoreAllMocks();
		mocks.exportBackup.mockReset();
		mocks.toastError.mockReset();
		mocks.anchorClick.mockReset();
	});

	it("渲染标题与导出/恢复两个按钮", () => {
		renderWithProvider(<BackupDialog open onOpenChange={vi.fn()} />);
		expect(screen.getByText("备份与恢复")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: /导出备份/ })).toBeInTheDocument();
		expect(screen.getByRole("button", { name: /恢复备份/ })).toBeInTheDocument();
	});

	it("点击导出备份触发下载", async () => {
		stubDownload();
		mocks.exportBackup.mockResolvedValue({
			version: 1,
			providers: [],
			virtualModels: [],
			apiKeys: [],
			settings: [],
		});
		renderWithProvider(<BackupDialog open onOpenChange={vi.fn()} />);
		fireEvent.click(screen.getByRole("button", { name: /导出备份/ }));

		await waitFor(() => {
			expect(mocks.exportBackup).toHaveBeenCalledTimes(1);
			expect(mocks.anchorClick).toHaveBeenCalledTimes(1);
		});
	});

	it("导出失败时 toast 错误", async () => {
		mocks.exportBackup.mockRejectedValue(new Error("network"));
		renderWithProvider(<BackupDialog open onOpenChange={vi.fn()} />);
		fireEvent.click(screen.getByRole("button", { name: /导出备份/ }));

		await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
	});

	it("点击恢复备份：关闭备份弹窗并打开导入弹窗", () => {
		renderWithProvider(<BackupHarness />);
		// 弹窗内含一个「恢复备份」按钮；导入弹窗关闭时不渲染标题。
		fireEvent.click(screen.getByRole("button", { name: /恢复备份/ }));

		// 导入弹窗标题出现（此时备份弹窗内容已卸载，两个同文案元素不再并存）。
		expect(screen.getByRole("heading", { name: "恢复备份" })).toBeInTheDocument();
	});
});

describe("ImportDialog", () => {
	beforeEach(() => {
		vi.restoreAllMocks();
		mocks.importBackup.mockReset();
		mocks.toastSuccess.mockReset();
		mocks.invalidate.mockReset();
	});

	it("未选择文件时确认导入按钮禁用", () => {
		renderWithProvider(<ImportDialog open onOpenChange={vi.fn()} />);
		expect(screen.getByRole("button", { name: "确认导入" })).toBeDisabled();
	});

	it("通过拖拽选择文件后显示文件名", async () => {
		renderWithProvider(<ImportDialog open onOpenChange={vi.fn()} />);
		const dropzone = screen.getByRole("button", { name: "拖拽文件到此处" });
		const file = new File([MINIMAL_JSON], "backup.json", { type: "application/json" });
		fireEvent.drop(dropzone, { dataTransfer: { files: [file] } });

		await waitFor(() => expect(screen.getByText(/backup\.json/)).toBeInTheDocument());
		expect(screen.getByRole("button", { name: "确认导入" })).not.toBeDisabled();
	});

	it("通过手动选择文件后显示文件名并启用确认", async () => {
		renderWithProvider(<ImportDialog open onOpenChange={vi.fn()} />);
		await attachFile(MINIMAL_JSON);
		expect(screen.getByRole("button", { name: "确认导入" })).not.toBeDisabled();
	});

	it("确认导入成功：Toast 成功、关闭弹窗、失效查询", async () => {
		const onOpenChange = vi.fn();
		mocks.importBackup.mockResolvedValue({
			providers: 0,
			models: 0,
			virtualModels: 0,
			apiKeys: 0,
			settings: 0,
		});
		renderWithProvider(<ImportDialog open onOpenChange={onOpenChange} />);
		await attachFile(MINIMAL_JSON);

		fireEvent.click(screen.getByRole("button", { name: "确认导入" }));
		// 先弹破坏性确认。
		expect(await screen.findByRole("heading", { name: "确认恢复备份" })).toBeInTheDocument();
		clickConfirmDialogAction();
		await waitFor(() => {
			expect(mocks.importBackup).toHaveBeenCalledWith(MINIMAL_JSON);
			expect(mocks.toastSuccess).toHaveBeenCalledWith("成功导入");
			expect(onOpenChange).toHaveBeenCalledWith(false);
			expect(mocks.invalidate).toHaveBeenCalled();
		});
	});

	it("确认导入失败：弹具体错误弹窗", async () => {
		const onOpenChange = vi.fn();
		const err = new Error("providers[0].name 重复：openai");
		mocks.importBackup.mockRejectedValue(err);
		renderWithProvider(<ImportDialog open onOpenChange={onOpenChange} />);
		await attachFile(MINIMAL_JSON);

		fireEvent.click(screen.getByRole("button", { name: "确认导入" }));
		expect(await screen.findByRole("heading", { name: "确认恢复备份" })).toBeInTheDocument();
		clickConfirmDialogAction();

		await screen.findByRole("heading", { name: "恢复失败" });
		expect(screen.getByText("providers[0].name 重复：openai")).toBeInTheDocument();
		expect(onOpenChange).not.toHaveBeenCalled();
	});
});
