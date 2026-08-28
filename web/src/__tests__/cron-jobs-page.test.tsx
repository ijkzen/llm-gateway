import type { CronJob } from "@/hooks/use-cron-jobs";
import CronJobsPage from "@/pages/cron-jobs";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

interface CapturedListProps {
	jobs: CronJob[] | undefined;
	selectedName: string | null;
	onSelect: (job: CronJob) => void;
}

interface CapturedDetailProps {
	job: CronJob | undefined;
	onEdit: (job: CronJob) => void;
	onDelete: (name: string) => void;
}

const mocks = vi.hoisted(() => {
	return {
		jobs: undefined as CronJob[] | undefined,
		isLoading: false,
		listProps: [] as CapturedListProps[],
		detailProps: [] as CapturedDetailProps[],
	};
});

vi.mock("@/hooks/use-cron-jobs", () => ({
	useCronJobs: () => ({
		data: mocks.jobs,
		isLoading: mocks.isLoading,
		isError: false,
		refetch: vi.fn(),
	}),
	useUpdateCronJob: () => ({ mutate: vi.fn(), isPending: false }),
	useRunCronJob: () => ({ mutate: vi.fn(), isPending: false }),
	useDeleteCronJob: () => ({ mutate: vi.fn(), isPending: false }),
}));

vi.mock("@/components/cron-jobs/CronJobList", () => ({
	CronJobList: (props: CapturedListProps) => {
		mocks.listProps.push(props);
		return (
			<div>
				{(props.jobs ?? []).map((job) => (
					<button key={job.name} type="button" onClick={() => props.onSelect(job)}>
						{job.name}
					</button>
				))}
			</div>
		);
	},
}));

vi.mock("@/components/cron-jobs/CronJobDetail", () => ({
	CronJobDetail: (props: CapturedDetailProps) => {
		mocks.detailProps.push(props);
		return <div data-testid="cron-job-detail" />;
	},
}));

vi.mock("@/components/cron-jobs/CronJobEditDialog", () => ({
	CronJobEditDialog: () => null,
}));

vi.mock("@/components/cron-jobs/CronJobDeleteDialog", () => ({
	CronJobDeleteDialog: () => null,
}));

vi.mock("@/components/cron-jobs/CronJobLogsDialog", () => ({
	CronJobLogsDialog: () => null,
}));

function makeJob(name: string, overrides: Partial<CronJob> = {}): CronJob {
	return {
		name,
		title: `${name} title`,
		description: "",
		expression: "0 0 8 * * *",
		enabled: true,
		group: "",
		last_run_at: "",
		next_run_at: "",
		updated_at: "",
		frequency_secs: 86400,
		...overrides,
	};
}

function lastListProps(): CapturedListProps {
	const props = mocks.listProps[mocks.listProps.length - 1];
	if (!props) throw new Error("CronJobList has not been rendered");
	return props;
}

function lastDetailProps(): CapturedDetailProps {
	const props = mocks.detailProps[mocks.detailProps.length - 1];
	if (!props) throw new Error("CronJobDetail has not been rendered");
	return props;
}

describe("CronJobsPage selected job", () => {
	beforeEach(() => {
		mocks.jobs = undefined;
		mocks.isLoading = false;
		mocks.listProps.length = 0;
		mocks.detailProps.length = 0;
	});

	it("passes the refreshed job object to CronJobDetail after list data updates", () => {
		const original = makeJob("a", { enabled: true });
		mocks.jobs = [original, makeJob("b")];
		const { rerender } = render(<CronJobsPage />);

		fireEvent.click(screen.getByRole("button", { name: "a" }));
		expect(lastListProps().selectedName).toBe("a");
		expect(lastDetailProps().job).toBe(original);

		const updated = makeJob("a", { enabled: false, title: "a new title" });
		mocks.jobs = [updated, makeJob("b")];
		rerender(<CronJobsPage />);

		expect(lastDetailProps().job).toBe(updated);
		expect(lastDetailProps().job?.enabled).toBe(false);
		expect(lastDetailProps().job?.title).toBe("a new title");
	});

	it("clears the detail panel when the selected job disappears from the list", () => {
		const original = makeJob("a");
		mocks.jobs = [original];
		const { rerender } = render(<CronJobsPage />);

		fireEvent.click(screen.getByRole("button", { name: "a" }));
		expect(lastDetailProps().job).toBe(original);

		mocks.jobs = [makeJob("b")];
		rerender(<CronJobsPage />);

		expect(lastDetailProps().job).toBeUndefined();
	});

	it("does not keep a stale job when jobs become undefined", () => {
		const original = makeJob("a");
		mocks.jobs = [original];
		const { rerender } = render(<CronJobsPage />);

		fireEvent.click(screen.getByRole("button", { name: "a" }));
		expect(lastDetailProps().job).toBe(original);

		mocks.jobs = undefined;
		rerender(<CronJobsPage />);

		expect(lastDetailProps().job).toBeUndefined();
	});

	it("renders neither list nor detail while loading", () => {
		mocks.isLoading = true;
		render(<CronJobsPage />);

		expect(mocks.listProps).toHaveLength(0);
		expect(mocks.detailProps).toHaveLength(0);
	});
});
