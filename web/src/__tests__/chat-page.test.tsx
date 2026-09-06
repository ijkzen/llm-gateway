import ChatPage from "@/pages/chat";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	providers: [
		{ id: 1, name: "供应商A", enable: true },
		{ id: 2, name: "供应商B", enable: false },
	],
	providerModels: [
		{ modelId: 11, providerId: 1, providerModelId: "model-a1" },
		{ modelId: 12, providerId: 2, providerModelId: "model-b1" },
	],
}));

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useProviders: () => ({ data: mocks.providers, isLoading: false, isError: false }),
	};
});

vi.mock("@/hooks/use-provider-models", async () => {
	const actual = await vi.importActual<typeof import("@/hooks/use-provider-models")>(
		"@/hooks/use-provider-models",
	);
	return {
		...actual,
		useProviderModels: () => ({ data: mocks.providerModels, isLoading: false, isError: false }),
	};
});

/** 构造 SSE 流式 fetch mock：chunks 逐段入队（stepMs 控制帧间隔），响应 abort 信号（模拟浏览器行为）。 */
function mockStreamingFetch(chunks: string[], holdOpen = false, stepMs = 10) {
	let streamController: ReadableStreamDefaultController<Uint8Array> | null = null;
	const fetchMock = vi.fn().mockImplementation((_url: string, init?: RequestInit) => {
		const stream = new ReadableStream<Uint8Array>({
			start(controller) {
				streamController = controller;
				let delay = 0;
				for (const chunk of chunks) {
					delay += stepMs;
					setTimeout(() => controller.enqueue(new TextEncoder().encode(chunk)), delay);
				}
				if (!holdOpen) {
					setTimeout(() => controller.close(), delay + 20);
				}
				init?.signal?.addEventListener("abort", () => {
					controller.error(new DOMException("Aborted", "AbortError"));
				});
			},
		});
		return Promise.resolve(
			new Response(stream, { status: 200, headers: { "content-type": "text/event-stream" } }),
		);
	});
	return {
		fetchMock,
		closeStream: () => {
			try {
				streamController?.close();
			} catch {
				// 流已因 abort 关闭
			}
		},
	};
}

function sseFrame(delta: Record<string, unknown>): Uint8Array {
	return new TextEncoder().encode(`data: ${JSON.stringify({ choices: [{ delta }] })}\n\n`);
}

function renderPage() {
	return render(
		<MemoryRouter>
			<ChatPage />
		</MemoryRouter>,
	);
}

/** 经浮窗选择模型并发送一条消息。 */
function selectModelAndSend(text: string) {
	openModelPickerAndSelect("model-a1");
	const input = screen.getByPlaceholderText("输入消息…");
	fireEvent.change(input, { target: { value: text } });
	fireEvent.click(screen.getByRole("button", { name: "发送" }));
}

/** 打开模型选择浮窗并选中指定模型条目（条目名只含模型 ID）。 */
function openModelPickerAndSelect(label: string) {
	fireEvent.click(screen.getByRole("button", { name: /^(选择模型|供应商A \/ model-a1)$/ }));
	fireEvent.click(screen.getByRole("button", { name: label }));
}

describe("ChatPage", () => {
	beforeEach(() => {
		vi.restoreAllMocks();
	});

	it("流式渲染思考与正文，思考完毕自动折叠且可手动展开", async () => {
		const { fetchMock } = mockStreamingFetch(
			[
				new TextDecoder().decode(sseFrame({ role: "assistant" })),
				new TextDecoder().decode(sseFrame({ reasoning_content: "先想想" })),
				// OpenRouter/Command Code 风格的思考键名同样要渲染。
				new TextDecoder().decode(sseFrame({ reasoning: "换个键名也想" })),
				new TextDecoder().decode(sseFrame({ content: "你好" })),
				new TextDecoder().decode(sseFrame({})),
				"data: [DONE]\n\n",
			],
			false,
			120,
		);
		vi.stubGlobal("fetch", fetchMock);
		renderPage();
		selectModelAndSend("嗨");

		// 用户气泡靠右、助手气泡出现。
		await waitFor(() => expect(screen.getByText("嗨")).toBeInTheDocument());
		// 思考过程流式可见：两种键名的增量都累计。
		await waitFor(() => expect(screen.getByText(/先想想换个键名也想/)).toBeInTheDocument());
		// 正文到达后思考自动折叠：思考文本不可见，但可通过手动展开回看。
		await waitFor(() => expect(screen.getByText("你好")).toBeInTheDocument());
		await waitFor(() => expect(screen.queryByText(/先想想/)).not.toBeInTheDocument());
		fireEvent.click(screen.getByRole("button", { name: /思考过程/ }));
		expect(screen.getByText(/先想想换个键名也想/)).toBeInTheDocument();
		// 请求体：直连端点、供应商与模型 ID、多轮消息。
		const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
		const body = JSON.parse(String(init.body));
		expect(init.signal).toBeDefined();
		expect(body).toEqual({
			providerId: 1,
			modelId: 11,
			messages: [{ role: "user", content: "嗨" }],
		});
	});

	it("reasoning_details 载体随流收集并随历史原样回传", async () => {
		const detail = {
			type: "reasoning.text",
			text: "想",
			signature: "sig-1",
			id: null,
			format: "anthropic-claude-v1",
			index: 0,
		};
		const { fetchMock } = mockStreamingFetch([
			new TextDecoder().decode(sseFrame({ role: "assistant" })),
			new TextDecoder().decode(sseFrame({ reasoning_content: "想", reasoning_details: [detail] })),
			new TextDecoder().decode(sseFrame({ content: "答" })),
			"data: [DONE]\n\n",
		]);
		vi.stubGlobal("fetch", fetchMock);
		renderPage();
		selectModelAndSend("问");
		await waitFor(() => expect(screen.getByText("答")).toBeInTheDocument());
		await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument());

		// 第二轮发送：历史里 assistant 消息原样携带 reasoning_details。
		const second = mockStreamingFetch([
			new TextDecoder().decode(sseFrame({ content: "再答" })),
			"data: [DONE]\n\n",
		]);
		vi.stubGlobal("fetch", second.fetchMock);
		fireEvent.change(screen.getByPlaceholderText("输入消息…"), { target: { value: "再问" } });
		fireEvent.click(screen.getByRole("button", { name: "发送" }));
		await waitFor(() => expect(screen.getByText("再答")).toBeInTheDocument());
		const [, init] = second.fetchMock.mock.calls[0] as [string, RequestInit];
		const body = JSON.parse(String(init.body));
		expect(body.messages).toEqual([
			{ role: "user", content: "问" },
			{ role: "assistant", content: "答", reasoning_details: [detail] },
			{ role: "user", content: "再问" },
		]);
	});

	it("发送中可停止，已收内容保留并标注停止", async () => {
		const { fetchMock, closeStream } = mockStreamingFetch(
			[new TextDecoder().decode(sseFrame({ content: "部分" }))],
			true,
		);
		vi.stubGlobal("fetch", fetchMock);
		renderPage();
		selectModelAndSend("嗨");
		await waitFor(() => expect(screen.getByText("部分")).toBeInTheDocument());
		fireEvent.click(screen.getByRole("button", { name: "停止" }));
		await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument());
		expect(screen.getByText("部分")).toBeInTheDocument();
		expect(screen.getByText(/已停止/)).toBeInTheDocument();
		closeStream();
	});

	it("清空对话重置消息列表", async () => {
		const { fetchMock } = mockStreamingFetch([
			new TextDecoder().decode(sseFrame({ content: "答" })),
			"data: [DONE]\n\n",
		]);
		vi.stubGlobal("fetch", fetchMock);
		renderPage();
		selectModelAndSend("问");
		await waitFor(() => expect(screen.getByText("答")).toBeInTheDocument());
		// 等流结束（停止按钮变回发送）再清空，避免点击到禁用按钮。
		await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument());
		fireEvent.click(screen.getByRole("button", { name: "清空对话" }));
		expect(screen.queryByText("问")).not.toBeInTheDocument();
		expect(screen.queryByText("答")).not.toBeInTheDocument();
	});

	it("模型浮窗按供应商分组，仅列启用供应商且选中态回显", () => {
		renderPage();
		fireEvent.click(screen.getByRole("button", { name: "选择模型" }));
		// 分组标题：启用供应商出现、停用供应商不出现。
		expect(screen.getByText("供应商A")).toBeInTheDocument();
		expect(screen.queryByText("供应商B")).not.toBeInTheDocument();
		expect(screen.queryByText("model-b1")).not.toBeInTheDocument();
		// 分组内条目只显示模型 ID，不带供应商名。
		expect(screen.getByRole("button", { name: "model-a1" })).toBeInTheDocument();
		expect(screen.queryByRole("button", { name: "供应商A / model-a1" })).not.toBeInTheDocument();
		// 点击分组标题折叠：条目隐藏，再点展开恢复。
		fireEvent.click(screen.getByRole("button", { name: /供应商A/ }));
		expect(screen.queryByRole("button", { name: "model-a1" })).not.toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: /供应商A/ }));
		expect(screen.getByRole("button", { name: "model-a1" })).toBeInTheDocument();
		// 选中后浮窗关闭，触发器回显「供应商 / 模型」。
		fireEvent.click(screen.getByRole("button", { name: "model-a1" }));
		expect(screen.getByRole("button", { name: "供应商A / model-a1" })).toBeInTheDocument();
	});

	it("请求失败在气泡内展示错误信息", async () => {
		vi.stubGlobal(
			"fetch",
			vi.fn().mockResolvedValue(
				new Response(JSON.stringify({ code: "UPSTREAM_ERROR", msg: "上游挂了" }), {
					status: 502,
				}),
			),
		);
		renderPage();
		selectModelAndSend("嗨");
		await waitFor(() => expect(screen.getByText(/上游挂了/)).toBeInTheDocument());
	});
});
