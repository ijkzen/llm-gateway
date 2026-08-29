import { DataTableToolbar } from "@/components/data-table-toolbar";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { SearchInput } from "@/components/search-input";
import { ChangePasswordDialog } from "@/components/settings/ChangePasswordDialog";
import { SettingEditDialog } from "@/components/settings/SettingEditDialog";
import { SettingsTable } from "@/components/settings/SettingsTable";
import { TableSkeleton } from "@/components/table-skeleton";
import { Button } from "@/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { type Setting, useSettings } from "@/hooks/use-settings";
import { SETTING_TYPES } from "@/lib/constants";
import { SETTINGS_PAGE } from "@/lib/pages";
import { KeyRound } from "lucide-react";
import { useMemo, useState } from "react";

export default function SettingsPage() {
	const [editingSetting, setEditingSetting] = useState<Setting | null>(null);
	const [changePasswordOpen, setChangePasswordOpen] = useState(false);
	const [searchQuery, setSearchQuery] = useState("");
	const [typeFilter, setTypeFilter] = useState("all");

	const { data: settings, isLoading, isError, refetch } = useSettings();

	const filteredSettings = useMemo(() => {
		let list = settings ?? [];
		if (searchQuery.trim()) {
			const q = searchQuery.toLowerCase();
			list = list.filter(
				(s) => s.key.toLowerCase().includes(q) || s.value.toLowerCase().includes(q),
			);
		}
		if (typeFilter !== "all") {
			list = list.filter((s) => s.type === typeFilter);
		}
		return list;
	}, [settings, searchQuery, typeFilter]);

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
				<PageHeader icon={SETTINGS_PAGE.icon} title={SETTINGS_PAGE.title} />
				<ErrorState
					description="无法获取系统设置数据，请检查网络或稍后重试。"
					onRetry={() => refetch()}
				/>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader icon={SETTINGS_PAGE.icon} title={SETTINGS_PAGE.title}>
				<Button variant="outline" onClick={() => setChangePasswordOpen(true)}>
					<KeyRound className="size-4" />
					修改密码
				</Button>
			</PageHeader>

			<DataTableToolbar>
				<SearchInput value={searchQuery} onChange={setSearchQuery} placeholder="搜索键或值..." />
				<Select value={typeFilter} onValueChange={setTypeFilter}>
					<SelectTrigger className="w-[160px]" aria-label="按类型筛选">
						<SelectValue placeholder="全部类型" />
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="all">全部类型</SelectItem>
						{SETTING_TYPES.map((type) => (
							<SelectItem key={type} value={type}>
								{type}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			</DataTableToolbar>

			<SettingsTable settings={filteredSettings} onEdit={setEditingSetting} />

			<SettingEditDialog
				setting={editingSetting}
				open={!!editingSetting}
				onOpenChange={(open) => !open && setEditingSetting(null)}
			/>

			<ChangePasswordDialog open={changePasswordOpen} onOpenChange={setChangePasswordOpen} />
		</div>
	);
}
