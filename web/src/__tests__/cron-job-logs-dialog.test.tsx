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

	it("渲染历史执行列表并可展开查看日志", () => {
		mocks.runs = [makeRun("run-1"), makeRun("run-2", { status: "failed", log_count: 3 })];
		mocks.runLogs["run-1"] = [makeLog(1, "INFO", "第一步"), makeLog(2, "WARN", "第二步")];
		renderDialog();
		emitIdle();

		// 两个执行条目 + 日志条数
		expect(screen.getByText("2 条日志")).toBeInTheDocument();
		expect(screen.getByText("3 条日志")).toBeInTheDocument();

		// 展开第一次执行
		fireEvent.click(screen.getByText("2 条日志"));
		expect(screen.getByText("第一步")).toBeInTheDocument();
		expect(screen.getByText("第二步")).toBeInTheDocument();
		expect(screen.getByText("WARN")).toBeInTheDocument();
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
});
