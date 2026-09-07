import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { BackupDialog } from "@/components/settings/BackupDialog";
import { ChangePasswordDialog } from "@/components/settings/ChangePasswordDialog";
import { JsonSettingEditDialog } from "@/components/settings/JsonSettingEditDialog";
import { SettingDeleteDialog } from "@/components/settings/SettingDeleteDialog";
import { SettingEditDialog } from "@/components/settings/SettingEditDialog";
import { SettingsTable } from "@/components/settings/SettingsTable";
import { TableSkeleton } from "@/components/table-skeleton";
import { Button } from "@/components/ui/button";
import { type Setting, useSettings } from "@/hooks/use-settings";
import { SETTINGS_PAGE } from "@/lib/pages";
import { ArchiveRestore, KeyRound } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

export default function SettingsPage() {
	const { t } = useTranslation();
	const [editingSetting, setEditingSetting] = useState<Setting | null>(null);
	const [deletingSetting, setDeletingSetting] = useState<Setting | null>(null);
	const [changePasswordOpen, setChangePasswordOpen] = useState(false);
	const [backupOpen, setBackupOpen] = useState(false);

	const { data: settings, isLoading, isError, refetch } = useSettings();

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
				<PageHeader icon={SETTINGS_PAGE.icon} title={t(SETTINGS_PAGE.titleKey)} />
				<ErrorState description={t("settings.errorDescription")} onRetry={() => refetch()} />
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader icon={SETTINGS_PAGE.icon} title={t(SETTINGS_PAGE.titleKey)}>
				<div className="flex items-center gap-2">
					<Button variant="outline" size="sm" onClick={() => setBackupOpen(true)}>
						<ArchiveRestore className="size-4" />
						{t("backup.title")}
					</Button>
					<Button variant="outline" size="sm" onClick={() => setChangePasswordOpen(true)}>
						<KeyRound className="size-4" />
						{t("settings.changePassword")}
					</Button>
				</div>
			</PageHeader>

			<SettingsTable settings={settings} onEdit={setEditingSetting} onDelete={setDeletingSetting} />

			{/* Json 类型走结构化表单弹窗（逐行增删键值），其余类型沿用单值编辑。 */}
			{editingSetting?.type === "Json" ? (
				<JsonSettingEditDialog
					setting={editingSetting}
					open={!!editingSetting}
					onOpenChange={(open) => !open && setEditingSetting(null)}
				/>
			) : (
				<SettingEditDialog
					setting={editingSetting}
					open={!!editingSetting}
					onOpenChange={(open) => !open && setEditingSetting(null)}
				/>
			)}

			<SettingDeleteDialog
				setting={deletingSetting}
				open={!!deletingSetting}
				onOpenChange={(open) => !open && setDeletingSetting(null)}
			/>

			<ChangePasswordDialog open={changePasswordOpen} onOpenChange={setChangePasswordOpen} />

			<BackupDialog open={backupOpen} onOpenChange={setBackupOpen} />
		</div>
	);
}
