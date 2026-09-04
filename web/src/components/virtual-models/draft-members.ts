import type { ProviderModel } from "@/hooks/use-provider-models";

/** 暂存成员：打开弹窗时回填既有成员（保留 virtualModelItemId 作 LB 顺序键）；暂存新增项该值为空。 */
export interface DraftItem {
	/** LB 顺序键：已保存成员为后端 virtual_model_item_id；暂存新增项为空，视为最大排同状态末尾。 */
	virtualModelItemId: number | null;
	modelId: number;
	enable: boolean;
}

export interface DraftMember {
	draft: DraftItem;
	model: ProviderModel;
}

/** 成员是否实际可用：成员自身启用且供应商启用。 */
export function draftUsable(draft: DraftItem, providerEnable: boolean): boolean {
	return draft.enable && providerEnable;
}

/**
 * 供应商组内成员排序：可用（启用成员且供应商启用）在前、不可用在后；
 * 同状态内按 virtualModelItemId 升序（与运行时 LB 基础顺序一致，
 * `load_members` 按 virtual_model_item_id 升序取启用成员）。
 * 暂存新增项没有 id，视为最大，排同状态内最后（新加模型加入后 id 最大，LB 也最后）。
 */
export function compareDraftMembers(
	a: DraftMember,
	b: DraftMember,
	providerEnableOf: (model: ProviderModel) => boolean,
): number {
	const aUsable = draftUsable(a.draft, providerEnableOf(a.model));
	const bUsable = draftUsable(b.draft, providerEnableOf(b.model));
	if (aUsable !== bUsable) return aUsable ? -1 : 1;
	const aKey = a.draft.virtualModelItemId ?? Number.MAX_SAFE_INTEGER;
	const bKey = b.draft.virtualModelItemId ?? Number.MAX_SAFE_INTEGER;
	return aKey - bKey;
}
