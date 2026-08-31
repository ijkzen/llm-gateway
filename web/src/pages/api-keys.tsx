import { ApiKeyCreateDialog } from "@/components/api-keys/ApiKeyCreateDialog";
import { ApiKeyDeleteDialog } from "@/components/api-keys/ApiKeyDeleteDialog";
import { ApiKeysTable } from "@/components/api-keys/ApiKeysTable";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { TableSkeleton } from "@/components/table-skeleton";
import { Button } from "@/components/ui/button";
import type { ApiKey } from "@/hooks/use-api-keys";
import { useApiKeys } from "@/hooks/use-api-keys";
import { API_KEYS_PAGE } from "@/lib/pages";
import { Plus, RefreshCw } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

export default function ApiKeysPage() {
	const { t } = useTranslation();
	const [creating, setCreating] = useState(false);
	const [deletingKey, setDeletingKey] = useState<ApiKey | null>(null);

	const { data: apiKeys, isLoading, isError, refetch } = useApiKeys();

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<TableSkeleton columns={5} rows={5} />
			</div>
		);
	}

	if (isError) {
		return (
			<div className="space-y-6">
				<PageHeader icon={API_KEYS_PAGE.icon} title={t(API_KEYS_PAGE.titleKey)} />
				<ErrorState description={t("apiKeys.errorDescription")} onRetry={() => refetch()} />
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader icon={API_KEYS_PAGE.icon} title={t(API_KEYS_PAGE.titleKey)}>
				<Button variant="outline" size="sm" onClick={() => refetch()}>
					<RefreshCw className="mr-2 size-4" />
					{t("common.refresh")}
				</Button>
				<Button size="sm" onClick={() => setCreating(true)}>
					<Plus className="mr-2 size-4" />
					{t("apiKeys.create")}
				</Button>
			</PageHeader>

			<ApiKeysTable apiKeys={apiKeys} onDelete={setDeletingKey} />

			<ApiKeyCreateDialog open={creating} onOpenChange={setCreating} />

			<ApiKeyDeleteDialog
				apiKey={deletingKey}
				open={!!deletingKey}
				onOpenChange={(open) => !open && setDeletingKey(null)}
			/>
		</div>
	);
}
