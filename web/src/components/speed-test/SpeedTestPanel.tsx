import { EmptyState } from "@/components/empty-state";
import { SpeedTestCandidates } from "@/components/speed-test/SpeedTestCandidates";
import { SpeedTestFilters } from "@/components/speed-test/SpeedTestFilters";
import { SpeedTestSummary } from "@/components/speed-test/SpeedTestSummary";
import { SpeedTestTable } from "@/components/speed-test/SpeedTestTable";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
	type SpeedTestFilter,
	useCancelSpeedTest,
	useSpeedTestCandidates,
	useSpeedTestRun,
	useStartSpeedTest,
} from "@/hooks/use-speed-test";
import { useToastActions } from "@/hooks/use-toast";
import { Gauge } from "lucide-react";
import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

/** URL query 中过滤条件的键（与请求日志页同一套「可分享视图」约定）。 */
const QUERY_PROVIDER_IDS = "providerIds";
const QUERY_VM_IDS = "vmIds";
const QUERY_KEYWORD = "keyword";

function parseIds(raw: string | null): number[] {
	if (!raw) return [];
	return raw
		.split(",")
		.map((part) => Number(part.trim()))
		.filter((value) => Number.isInteger(value) && value > 0);
}

/**
 * 模型测速面板：过滤栏 + 汇总条 + 结果表。
 *
 * 状态真相在服务端：切页面回来、刷新、分享链接都能看到同一次运行的进度/成绩，
 * 前端不持有任何测速中间态。过滤条件写进 URL query（可分享、可后退）。
 *
 * 三态渲染（优先级从高到低）：
 * - **测速中**：展示进度与逐行结果（哪怕筛选已改，也先让人看到正在跑的东西）。
 * - **结果属于当前筛选**：展示成绩（筛选与运行时快照一致）。
 * - **其余**：展示候选清单——筛选一变化就列出「将会测谁」，不必等到测速。
 */
export function SpeedTestPanel() {
	const { t } = useTranslation();
	const { toastError } = useToastActions();
	const [searchParams, setSearchParams] = useSearchParams();

	const filter = useMemo<SpeedTestFilter>(
		() => ({
			providerIds: parseIds(searchParams.get(QUERY_PROVIDER_IDS)),
			vmIds: parseIds(searchParams.get(QUERY_VM_IDS)),
			keyword: searchParams.get(QUERY_KEYWORD) ?? "",
		}),
		[searchParams],
	);

	const setFilter = useCallback(
		(next: SpeedTestFilter) => {
			const params = new URLSearchParams(searchParams);
			const write = (key: string, value: string) => {
				if (value) {
					params.set(key, value);
				} else {
					params.delete(key);
				}
			};
			write(QUERY_PROVIDER_IDS, next.providerIds.join(","));
			write(QUERY_VM_IDS, next.vmIds.join(","));
			write(QUERY_KEYWORD, next.keyword.trim());
			setSearchParams(params, { replace: true });
		},
		[searchParams, setSearchParams],
	);

	const candidates = useSpeedTestCandidates(filter);
	const runQuery = useSpeedTestRun();
	const startMutation = useStartSpeedTest();
	const cancelMutation = useCancelSpeedTest();

	const run = runQuery.data?.run ?? null;
	const rows = runQuery.data?.results ?? [];
	const running = run?.running ?? false;

	// 结果是否属于当前筛选：与运行快照逐项比对（id 列表按集合比，避免顺序差异）。
	const matchesFilter =
		run !== null &&
		sameIds(run.providerIds, filter.providerIds) &&
		sameIds(run.vmIds, filter.vmIds) &&
		run.keyword.trim() === filter.keyword.trim();

	const candidateModels = candidates.data?.models ?? [];

	const handleStart = () => {
		startMutation.mutate(filter, {
			onSuccess: (result) => {
				if (result.alreadyRunning) {
					// 已有运行在跑：不报错，直接展示进度（用户连点两下不该看到红色提示）。
					void runQuery.refetch();
				}
			},
			onError: (error) => toastError(t("speedTest.startFailed"), error),
		});
	};

	const handleCancel = () => {
		cancelMutation.mutate(undefined, {
			onError: (error) => toastError(t("speedTest.cancelFailed"), error),
		});
	};

	const renderBody = () => {
		if (runQuery.isLoading) {
			return (
				<Card>
					<CardContent className="space-y-2 pt-6">
						<Skeleton className="h-6 w-64" />
						<Skeleton className="h-40 w-full" />
					</CardContent>
				</Card>
			);
		}
		if (runQuery.isError) {
			return (
				<EmptyState
					icon={Gauge}
					title={t("speedTest.loadFailed")}
					description={t("speedTest.loadFailedHint")}
				/>
			);
		}

		// 测速中优先：让人看到正在跑的进度，不因改筛选而消失。
		if (running && run) {
			return (
				<Card>
					<CardContent className="space-y-3 pt-6">
						<SpeedTestSummary run={run} rows={rows} />
						<p className="text-sm text-muted-foreground" data-testid="speed-test-progress">
							{t("speedTest.progress", {
								done: rows.filter((row) => row.status !== "pending").length,
								total: rows.length,
							})}
						</p>
						<SpeedTestTable rows={rows} />
					</CardContent>
				</Card>
			);
		}

		// 结果属于当前筛选：展示成绩。
		if (run && matchesFilter) {
			return (
				<Card>
					<CardContent className="space-y-3 pt-6">
						<SpeedTestSummary run={run} rows={rows} />
						<SpeedTestTable rows={rows} />
					</CardContent>
				</Card>
			);
		}

		// 其余：展示候选清单（筛选一变化即列出会测谁）。
		if (candidates.isLoading) {
			return (
				<Card>
					<CardContent className="space-y-2 pt-6">
						<Skeleton className="h-6 w-48" />
						<Skeleton className="h-32 w-full" />
					</CardContent>
				</Card>
			);
		}
		if (candidateModels.length === 0) {
			return (
				<EmptyState
					icon={Gauge}
					title={t("speedTest.noCandidatesTitle")}
					description={t("speedTest.noCandidatesHint")}
				/>
			);
		}
		return (
			<Card>
				<CardContent className="space-y-3 pt-6">
					<p className="text-sm text-muted-foreground" data-testid="speed-test-candidates-hint">
						{t("speedTest.candidatesHint", { count: candidateModels.length })}
					</p>
					<SpeedTestCandidates models={candidateModels} />
				</CardContent>
			</Card>
		);
	};

	return (
		<div className="space-y-4">
			<SpeedTestFilters
				filter={filter}
				onFilterChange={setFilter}
				candidateTotal={candidates.data?.total}
				running={running}
				starting={startMutation.isPending}
				onStart={handleStart}
				onCancel={handleCancel}
			/>
			{renderBody()}
		</div>
	);
}

/** 两个 id 列表是否表示同一集合（顺序无关、重复忽略）。 */
function sameIds(left: number[], right: number[]): boolean {
	const leftSet = new Set(left);
	const rightSet = new Set(right);
	if (leftSet.size !== rightSet.size) return false;
	for (const value of leftSet) {
		if (!rightSet.has(value)) return false;
	}
	return true;
}
