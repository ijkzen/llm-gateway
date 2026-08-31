import { PageHeader } from "@/components/page-header";
import { RequestLogsTable } from "@/components/request-logs/RequestLogsTable";
import { REQUEST_LOGS_PAGE } from "@/lib/pages";
import { useTranslation } from "react-i18next";

export default function RequestLogsPage() {
	const { t } = useTranslation();
	return (
		<div className="space-y-6">
			<PageHeader icon={REQUEST_LOGS_PAGE.icon} title={t(REQUEST_LOGS_PAGE.titleKey)} />
			<RequestLogsTable />
		</div>
	);
}
