import { CronJobLogsDialog } from "@/components/cron-jobs/CronJobLogsDialog";
import type { CronJobLog, CronJobRun } from "@/hooks/use-cron-job-logs";
import type { CronJob } from "@/hooks/use-cron-jobs";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	runs: [] as CronJobRun[],
	runLogs: {} as Record<string, CronJobLog[]>,
}));

// 数据 hooks 走 mock；SSE hook 用真实实现，由 MockEventSource 驱动事件。
vi.mock("@/hooks/use-cron-job-logs", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/hooks/use-cron-job-logs")>();
	return {
		...actual,
		useCronJobRuns: () => ({ data: mocks.runs }),
		useCronJobRunLogs: (_name: string, runId: string | null) => ({
			data: runId ? (mocks.runLogs[runId] ?? []) : [],
			isLoading: false,
		}),
	};
});

vi.mock("@/lib/api", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/lib/api")>();
	return {
		...actual,
		// reset 分支的 fetchQuery 走真实 fetchRunLogs（不经过被 mock 的 hook），
		// 把网络层替换成 mocks.runLogs 以驱动合并逻辑。
		api: {
			get: (path: string) => {
				const runId = path.split("/").pop() ?? "";
				return {
					json: async () => ({
						code: "0",
						msg: "ok",
						data: mocks.runLogs[runId] ?? [],
					}),
				};
			},
			post: () => ({ json: async () => ({ code: "0", msg: "ok" }) }),
			put: () => ({ json: async () => ({ code: "0", msg: "ok" }) }),
			delete: () => ({ json: async () => ({ code: "0", msg: "ok" }) }),
		},
	};
});

function makeLog(
	seq: number,
	level: "INFO" | "WARN" | "ERROR" = "INFO",
	message?: string,
): CronJobLog {
	return { seq, level, message: message ?? `日志 ${seq}`, ts: "2026-08-13T08:00:00Z" };
}

function makeRun(runId: string, overrides: Partial<CronJobRun> = {}): CronJobRun {
	return {
		run_id: runId,
		job_name: "example",
		status: "success",
		started_at: "2026-08-13T08:00:00Z",
		ended_at: "2026-08-13T08:00:05Z",
		log_count: 2,
		truncated: false,
		...overrides,
	};
}

function makeJob(): CronJob {
	return {
		name: "example",
		title: "示例任务",
		description: "",
		expression: "@hourly",
		enabled: true,
		group: "default",
		last_run_at: "2026-08-13T08:00:05Z",
		next_run_at: "2026-08-13T09:00:00Z",
		updated_at: "2026-08-13T08:00:00Z",
		frequency_secs: 3600,
	};
}

class MockEventSource {
	static instances: MockEventSource[] = [];
	onopen: (() => void) | null = null;
	onerror: (() => void) | null = null;
	url: string;
	closed = false;
	private listeners: Record<string, Array<(e: MessageEvent) => void>> = {};

	constructor(url: string) {
		this.url = url;
		MockEventSource.instances.push(this);
	}

	addEventListener(type: string, cb: (e: MessageEvent) => void) {
		if (!this.listeners[type]) {
			this.listeners[type] = [];
		}
		this.listeners[type].push(cb);
	}

	close() {
		this.closed = true;
	}

	emit(type: string, data: unknown) {
		for (const cb of this.listeners[type] ?? []) {
			cb({ data: JSON.stringify(data) } as MessageEvent);
		}
	}
}

function renderDialog() {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>
			<CronJobLogsDialog job={makeJob()} open onOpenChange={() => {}} />
		</QueryClientProvider>,
	);
}

/** 取最近一次创建的 EventSource 实例；未创建时给出明确失败信息。 */
function instance() {
	const es = MockEventSource.instances[0];
	if (!es) {
		throw new Error("MockEventSource instance not found");
	}
	return es;
}

/** 连接建立时无执行中的任务。 */
function emitIdle() {
	act(() => {
		instance().emit("idle", {});
	});
}

/** 连接建立时有执行中的任务，回放其日志。 */
function emitSnapshot(runId: string, logs: CronJobLog[]) {
	act(() => {
		instance().emit("snapshot", {
			run_id: runId,
			started_at: "2026-08-13T08:00:00Z",
			logs,
		});
	});
}

/** 推送一条实时日志事件。 */
function emitLog(runId: string, log: CronJobLog) {
	act(() => {
		instance().emit("log", {
			kind: "log",
			job_name: "example",
			run_id: runId,
			seq: log.seq,
			level: log.level,
			message: log.message,
			ts: log.ts,
		});
	});
}

/** 推送执行结束事件。 */
function emitRunEnded(runId: string, status: "success" | "failed", truncated: boolean) {
	act(() => {
		instance().emit("run_ended", {
			kind: "run_ended",
			job_name: "example",
			run_id: runId,
			status,
			truncated,
			ts: "2026-08-13T08:00:05Z",
		});
	});
}

describe("CronJobLogsDialog", () => {
	beforeEach(() => {
		mocks.runs = [];
		mocks.runLogs = {};
		MockEventSource.instances = [];
		vi.stubGlobal("EventSource", MockEventSource);
	});

	it("无任何日志时展示空态文案", () => {
		renderDialog();
		emitIdle();

		expect(screen.getByText("当前没有正在执行的任务")).toBeInTheDocument();
		expect(screen.getByText("该定时任务未输出日志")).toBeInTheDocument();
	});

	it("渲染历史执行列表，点开某次执行后日志显示在上区", () => {
		mocks.runs = [makeRun("run-1"), makeRun("run-2", { status: "failed", log_count: 3 })];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "第一步"), makeLog(2, "WARN", "第二步")];
		renderDialog();
		emitIdle();

		// 两个执行条目 + 日志条数
		expect(screen.getByText("2 条日志")).toBeInTheDocument();
		expect(screen.getByText("3 条日志")).toBeInTheDocument();

		// 点开第一次执行：日志显示在上区（与历史头部同一面板），不在历史列表行下方
		fireEvent.click(screen.getByText("2 条日志"));
		expect(screen.getByText("第一步")).toBeInTheDocument();
		expect(screen.getByText("第二步")).toBeInTheDocument();
		expect(screen.getByText("WARN")).toBeInTheDocument();

		const content = screen.getByText("第一步");
		expect(content.closest("ul")).toBeNull();
		expect(screen.getByText("历史执行日志").closest("div.rounded-lg")).toContainElement(content);
	});

	it("历史日志条目为两行：第一行时间+级别，第二行内容", () => {
		mocks.runs = [makeRun("run-1")];
		mocks.runLogs["run-1"] = [makeLog(1, "WARN", "第一步")];
		renderDialog();
		emitIdle();

		fireEvent.click(screen.getByText("2 条日志"));

		const content = screen.getByText("第一步");
		const entry = content.parentElement as HTMLElement;
		// 两行结构：第一行元信息（时间 + 级别），第二行内容。
		expect(entry.children).toHaveLength(2);
		expect(entry.children[0]).toHaveTextContent("WARN");
		expect(entry.children[0]).toHaveTextContent(/2026-08-1[34] \d{2}:\d{2}:\d{2}/);
		expect(entry.children[1]).toBe(content);
	});

	it("实时日志条目同样为两行：第一行时间+级别，第二行内容", () => {
		renderDialog();
		emitSnapshot("run-live", [makeLog(1, "ERROR", "出错了")]);

		const content = screen.getByText("出错了");
		const entry = content.parentElement as HTMLElement;
		expect(entry.children).toHaveLength(2);
		expect(entry.children[0]).toHaveTextContent("ERROR");
		expect(entry.children[0]).toHaveTextContent(/2026-08-1[34] \d{2}:\d{2}:\d{2}/);
		expect(entry.children[1]).toBe(content);
	});

	it("历史模式头部显示起止时间，不重复状态徽章与日志条数", () => {
		mocks.runs = [makeRun("run-1")];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "第一步")];
		renderDialog();
		emitIdle();

		fireEvent.click(screen.getByText("2 条日志"));

		expect(screen.getByText("历史执行日志")).toBeInTheDocument();
		expect(screen.queryByText("实时日志")).not.toBeInTheDocument();
		expect(screen.getByText("返回实时")).toBeInTheDocument();

		const header = screen.getByText("历史执行日志").parentElement as HTMLElement;
		expect(header).toHaveTextContent(
			/2026-08-1[34] \d{2}:\d{2}:\d{2} ~ 2026-08-1[34] \d{2}:\d{2}:\d{2}/,
		);
		// 状态徽章与日志条数只在历史行上出现一次，头部不再重复。
		expect(screen.getAllByText("成功")).toHaveLength(1);
		expect(screen.getAllByText("2 条日志")).toHaveLength(1);
	});

	it("点「返回实时」切回实时日志", () => {
		mocks.runs = [makeRun("run-1")];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "历史一")];
		renderDialog();
		emitSnapshot("run-live", [makeLog(1, "INFO", "实时一")]);

		fireEvent.click(screen.getByText("2 条日志"));
		expect(screen.getByText("历史一")).toBeInTheDocument();
		expect(screen.queryByText("实时一")).not.toBeInTheDocument();

		fireEvent.click(screen.getByText("返回实时"));
		expect(screen.getByText("实时日志")).toBeInTheDocument();
		expect(screen.getByText("实时一")).toBeInTheDocument();
		expect(screen.queryByText("历史一")).not.toBeInTheDocument();
	});

	it("从历史模式返回实时后滚到最新日志", () => {
		mocks.runs = [makeRun("run-1")];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "历史一")];
		renderDialog();
		emitSnapshot("run-live", [makeLog(1, "INFO", "实时一")]);

		// 实时与历史共用同一个滚动容器；jsdom 里 scrollHeight 默认为 0，需实测定义。
		const liveLogs = screen.getByText("实时一").closest(".overflow-y-auto") as HTMLElement;
		Object.defineProperty(liveLogs, "scrollHeight", { value: 500, configurable: true });

		fireEvent.click(screen.getByText("2 条日志"));
		// 历史模式下把同一容器滚离底部。
		liveLogs.scrollTop = 100;

		fireEvent.click(screen.getByText("返回实时"));
		expect(liveLogs.scrollTop).toBe(500);
	});

	it("查看历史期间新执行开始不自动切回，实时日志继续在后台接收", () => {
		mocks.runs = [makeRun("run-1")];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "历史一")];
		renderDialog();
		emitSnapshot("run-live", [makeLog(1, "INFO", "实时一")]);

		fireEvent.click(screen.getByText("2 条日志"));

		act(() => {
			instance().emit("run_started", {
				kind: "run_started",
				job_name: "example",
				run_id: "run-new",
				ts: "2026-08-13T09:00:00Z",
			});
		});

		// 仍停留在历史模式。
		expect(screen.getByText("历史执行日志")).toBeInTheDocument();
		expect(screen.queryByText("实时日志")).not.toBeInTheDocument();

		// 切回实时可见新执行的日志（说明历史模式下实时流仍在接收）。
		emitLog("run-new", makeLog(1, "INFO", "新执行一"));
		fireEvent.click(screen.getByText("返回实时"));
		expect(screen.getByText("新执行一")).toBeInTheDocument();
	});

	it("历史模式该次执行无日志时显示空态文案", () => {
		mocks.runs = [makeRun("run-1")];
		mocks.runLogs["run-1"] = [];
		renderDialog();
		emitIdle();

		fireEvent.click(screen.getByText("2 条日志"));

		const empty = screen.getByText("该次执行未输出日志");
		expect(empty).toBeInTheDocument();
		expect(empty.closest("ul")).toBeNull();
	});

	it("选中的历史行以 aria-pressed 暴露选中态并切换箭头方向", () => {
		mocks.runs = [makeRun("run-1"), makeRun("run-2", { log_count: 3 })];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "第一步")];
		renderDialog();
		emitIdle();

		const row1 = screen.getByText("2 条日志").closest("button") as HTMLElement;
		const row2 = screen.getByText("3 条日志").closest("button") as HTMLElement;
		expect(row1).toHaveAttribute("aria-pressed", "false");
		expect(row2).toHaveAttribute("aria-pressed", "false");
		expect(row1.querySelector(".lucide-chevron-right")).not.toBeNull();

		fireEvent.click(row1);

		expect(row1).toHaveAttribute("aria-pressed", "true");
		expect(row2).toHaveAttribute("aria-pressed", "false");
		expect(row1.querySelector(".lucide-chevron-down")).not.toBeNull();
		expect(row2.querySelector(".lucide-chevron-right")).not.toBeNull();
	});

	it("关闭弹窗后重新打开回到实时模式", () => {
		mocks.runs = [makeRun("run-1")];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "历史一")];
		const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
		const view = render(
			<QueryClientProvider client={queryClient}>
				<CronJobLogsDialog job={makeJob()} open onOpenChange={() => {}} />
			</QueryClientProvider>,
		);
		emitIdle();
		fireEvent.click(screen.getByText("2 条日志"));
		expect(screen.getByText("历史执行日志")).toBeInTheDocument();

		view.rerender(
			<QueryClientProvider client={queryClient}>
				<CronJobLogsDialog job={makeJob()} open={false} onOpenChange={() => {}} />
			</QueryClientProvider>,
		);
		view.rerender(
			<QueryClientProvider client={queryClient}>
				<CronJobLogsDialog job={makeJob()} open onOpenChange={() => {}} />
			</QueryClientProvider>,
		);

		expect(screen.getByText("实时日志")).toBeInTheDocument();
		expect(screen.queryByText("历史执行日志")).not.toBeInTheDocument();
	});

	it("实时日志带时间戳渲染，新日志实时追加", () => {
		renderDialog();
		emitSnapshot("run-live", [makeLog(1, "INFO", "执行中：第 1 步")]);

		expect(screen.getByText("执行中：第 1 步")).toBeInTheDocument();
		// 时间戳（本地时区格式化后为 YYYY-MM-DD HH:mm:ss）
		expect(screen.getAllByText(/2026-08-1[34] \d{2}:\d{2}:\d{2}/).length).toBeGreaterThan(0);

		// 实时追加一条新日志（run_ended 前的 log 事件）
		emitLog("run-live", makeLog(2, "ERROR", "出错了"));
		expect(screen.getByText("出错了")).toBeInTheDocument();
	});

	it("快照回放与实时事件重叠时按 seq 去重，新事件正常追加", () => {
		renderDialog();
		// E6「先订阅后快照」：快照已含 seq 1/2，订阅后同两条又经广播送达。
		emitSnapshot("run-dup", [makeLog(1, "INFO", "第一步"), makeLog(2, "INFO", "第二步")]);
		emitLog("run-dup", makeLog(1, "INFO", "第一步"));
		emitLog("run-dup", makeLog(2, "INFO", "第二步"));
		// 重叠事件被丢弃：各只渲染一条。
		expect(screen.getAllByText("第一步")).toHaveLength(1);
		expect(screen.getAllByText("第二步")).toHaveLength(1);

		// seq 更大的新事件正常追加。
		emitLog("run-dup", makeLog(3, "ERROR", "第三步出错"));
		expect(screen.getByText("第三步出错")).toBeInTheDocument();
	});

	it("执行结束后显示状态与截断提示，并刷新历史列表", () => {
		mocks.runs = [makeRun("run-1", { status: "failed", truncated: true })];
		renderDialog();
		emitSnapshot("run-1", [makeLog(1)]);

		emitRunEnded("run-1", "failed", true);

		expect(screen.getByText(/执行失败/)).toBeInTheDocument();
		expect(screen.getByText(/日志已达上限被截断/)).toBeInTheDocument();
	});

	it("向上滚动暂停自动跟随，回到底部或点击按钮恢复", () => {
		renderDialog();
		emitSnapshot("run-1", [makeLog(1), makeLog(2), makeLog(3)]);

		const liveLogs = screen.getByText("日志 1").closest(".overflow-y-auto");
		expect(liveLogs).not.toBeNull();
		const container = liveLogs as HTMLElement;

		// 默认处于底部：没有“回到最新”按钮。
		expect(screen.queryByText("回到最新")).not.toBeInTheDocument();

		// 模拟向上滚动：距离底部超过阈值 → 暂停跟随。
		Object.defineProperty(container, "scrollHeight", { value: 500, configurable: true });
		Object.defineProperty(container, "clientHeight", { value: 200, configurable: true });
		container.scrollTop = 100;
		fireEvent.scroll(container);
		expect(screen.getByText("回到最新")).toBeInTheDocument();

		// 点击“回到最新”：滚到底部并恢复跟随，按钮消失。
		fireEvent.click(screen.getByText("回到最新"));
		expect(container.scrollTop).toBe(container.scrollHeight);
		expect(screen.queryByText("回到最新")).not.toBeInTheDocument();

		// 回到底部（onScroll 触发）同样恢复跟随。
		container.scrollTop = 480;
		fireEvent.scroll(container);
		expect(container.scrollTop).toBe(container.scrollHeight);
		expect(screen.queryByText("回到最新")).not.toBeInTheDocument();
	});

	it("弹窗卸载时关闭 SSE 连接", () => {
		const { unmount } = renderDialog();
		expect(MockEventSource.instances).toHaveLength(1);
		expect(instance().url).toContain("/api/cron-jobs/example/logs/stream");

		unmount();
		expect(instance().closed).toBe(true);
	});

	it("run_started 清空旧日志并切换当前执行", () => {
		renderDialog();
		emitSnapshot("run-old", [makeLog(1, "INFO", "旧执行日志")]);
		expect(screen.getByText("旧执行日志")).toBeInTheDocument();

		act(() => {
			instance().emit("run_started", {
				kind: "run_started",
				job_name: "example",
				run_id: "run-new",
				ts: "2026-08-13T09:00:00Z",
			});
		});

		// 旧执行的日志被清空（新执行的实时区从空开始）。
		expect(screen.queryByText("旧执行日志")).not.toBeInTheDocument();

		// 新执行的日志正常接收。
		emitLog("run-new", makeLog(1, "INFO", "新执行日志"));
		expect(screen.getByText("新执行日志")).toBeInTheDocument();
	});

	it("reset 事件按 seq 合并重拉日志，不丢实时增量（18-02 回归）", async () => {
		mocks.runLogs["run-r"] = [makeLog(1, "INFO", "拉回的一"), makeLog(2, "INFO", "拉回的二")];
		renderDialog();
		emitSnapshot("run-r", [makeLog(1, "INFO", "拉回的一")]);
		// 拉取在途时又到了一条实时日志。
		emitLog("run-r", makeLog(3, "INFO", "在途实时三"));

		act(() => {
			instance().emit("reset", {});
		});

		// 合并结果：拉回的 1/2 + 本地更靠后的 3，且不重复。
		await screen.findByText("拉回的二");
		expect(screen.getByText("在途实时三")).toBeInTheDocument();
		expect(screen.getAllByText("拉回的一")).toHaveLength(1);
	});

	it("断开连接进入重连态（18-11 退避重连）", () => {
		renderDialog();
		emitSnapshot("run-x", [makeLog(1)]);

		act(() => {
			instance().onerror?.();
		});
		expect(screen.getByText("连接断开，正在重连…")).toBeInTheDocument();
	});

	it("多次重连失败后停止重试并提示刷新（18-11）", () => {
		vi.useFakeTimers();
		try {
			renderDialog();
			emitSnapshot("run-x", [makeLog(1)]);

			// 连续失败超过上限（5）：每次失败后推进退避计时器，新一轮重建后继续失败。
			for (let i = 0; i < 6; i += 1) {
				act(() => {
					const latest = MockEventSource.instances[MockEventSource.instances.length - 1];
					latest?.onerror?.();
				});
				act(() => {
					vi.advanceTimersByTime(60_000);
				});
			}

			expect(screen.getByText(/实时连接已中断/)).toBeInTheDocument();
		} finally {
			vi.useRealTimers();
		}
	});
});
