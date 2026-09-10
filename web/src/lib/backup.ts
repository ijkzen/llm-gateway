import { type ApiResponse, api, unwrap } from "@/lib/api";

export interface BackupModel {
	providerModelId: string;
	contextLength: number;
	maxOutputTokens: number;
	reasoning: boolean;
	toolUse: boolean;
	imageUnderstand: boolean;
	videoUnderstand: boolean;
	protocolType: number | null;
	proxyEnabled: boolean;
	proxyAddr: string;
}

export interface BackupProvider {
	name: string;
	enable: boolean;
	baseUrl: string;
	apiKey: string;
	customHeader: string;
	protocolType: number;
	billingMode: number;
	extra: string;
	sortOrder: number;
	proxyEnabled: boolean;
	proxyAddr: string;
	disabledReason: string | null;
	models: BackupModel[];
}

export interface BackupVirtualModelItem {
	providerName: string;
	providerModelId: string;
	enable: boolean;
	cascadeDisabled: boolean;
}

export interface BackupVirtualModel {
	displayId: string;
	enable: boolean;
	loadBalancingStrategy: number;
	fallbackStrategy: number;
	interfaceType: number;
	items: BackupVirtualModelItem[];
}

export interface BackupApiKey {
	name: string;
	key: string;
	enable: boolean;
}

export interface BackupSetting {
	key: string;
	value: string;
	type: string;
}

export interface BackupFile {
	version: number;
	exportedAt: string;
	providers: BackupProvider[];
	virtualModels: BackupVirtualModel[];
	apiKeys: BackupApiKey[];
	settings: BackupSetting[];
}

export interface ImportSummary {
	providers: number;
	models: number;
	virtualModels: number;
	apiKeys: number;
	settings: number;
}

/** 导出备份（全量配置 JSON，含明文密钥）。 */
export async function fetchBackupExport(): Promise<BackupFile> {
	const res = await api.get("backup/export").json<ApiResponse<BackupFile>>();
	return unwrap(res);
}

/** 恢复备份：整体替换导入。成功返回导入计数，失败抛 ApiError（msg 为具体中文错误）。 */
export async function importBackup(jsonText: string): Promise<ImportSummary> {
	const res = await api
		// 整库替换在库大时较慢，给足超时（19-04）。
		.post("backup/import", {
			body: jsonText,
			headers: { "content-type": "application/json" },
			timeout: 120000,
		})
		.json<ApiResponse<ImportSummary>>();
	return unwrap(res);
}
