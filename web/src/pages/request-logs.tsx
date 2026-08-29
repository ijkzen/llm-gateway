import { PageHeader } from "@/components/page-header";
import { RequestLogsTable } from "@/components/request-logs/RequestLogsTable";
import { REQUEST_LOGS_PAGE } from "@/lib/pages";

export default function RequestLogsPage() {
	return (
		<div className="space-y-6">
			<PageHeader icon={REQUEST_LOGS_PAGE.icon} title={REQUEST_LOGS_PAGE.title} />
			<RequestLogsTable />
		</div>
	);
}
