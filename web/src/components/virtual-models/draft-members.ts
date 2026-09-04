import type { ProviderModel } from "@/hooks/use-provider-models";

/** 暂存成员：打开弹窗时回填既有成员（virtualModelItemId 保留作顺序标识）；暂存新增项该值为空。 */
export interface DraftItem {
	/** 已保存成员为后端 virtual_model_item_id；暂存新增项为空。 */
	virtualModelItemId: number | null;
	modelId: number;
	enable: boolean;
}

export interface DraftMember {
	draft: DraftItem;
	model: ProviderModel;
}

/**
 * 供应商组内成员排序：仅按成员自身 enable 分两堆（启用在前、停用在后），
 * 同 enable 内保持数组原有相对顺序（稳定排序）。
 *
 * draftItems 初始化时直接取自后端返回的 items（已按虚拟模型用量感知 LB 序
 * 排列：enable → 策略分组 → 组内用量 → 平局 id 升序），因此组内行序即后端序，
 * 不做额外重排。暂存新增项（virtualModelItemId 为 null）追加在数组尾部，
 * 稳定排序后落在同 enable 段的末尾。
 */
export function compareDraftMembers(a: DraftMember, b: DraftMember): number {
	return Number(b.draft.enable) - Number(a.draft.enable);
}
