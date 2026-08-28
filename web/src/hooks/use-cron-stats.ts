import { useCronJobs } from "@/hooks/use-cron-jobs";
import { DEFAULT_GROUP } from "@/lib/constants";
import { useMemo } from "react";

export interface CronStats {
	total: number;
	enabled: number;
	groups: number;
}

export function useCronStats(): CronStats {
	const { data: jobs } = useCronJobs();
	return useMemo(() => {
		const all = jobs ?? [];
		const enabled = all.filter((j) => j.enabled).length;
		const groups = new Set(all.map((j) => j.group || DEFAULT_GROUP)).size;
		return { total: all.length, enabled, groups };
	}, [jobs]);
}
