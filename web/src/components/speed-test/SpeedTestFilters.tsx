import { MultiSelect, type MultiSelectOption } from "@/components/multi-select";
import { SearchInput } from "@/components/search-input";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useProviders } from "@/hooks/use-providers";
import type { SpeedTestFilter } from "@/hooks/use-speed-test";
import { useVirtualModels } from "@/hooks/use-virtual-models";
import { Gauge, Loader2, Square } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

interface SpeedTestFiltersProps {
	filter: SpeedTestFilter;
	onFilterChange: (filter: SpeedTestFilter) => void;
	/** 候选总数（「将测 N 个模型」）；undefined = 正在算。 */
	candidateTotal?: number;
	running: boolean;
	starting: boolean;
	onStart: () => void;
	onCancel: () => void;
}

/**
 * 过滤栏 + 主操作按钮。
 *
 * 过滤只影响「下一次运行」的候选集与计数；结果表固定展示该次运行的全部行，
 * 不受当前过滤影响（改过滤不会让已有成绩凭空消失）。
 */
export function SpeedTestFilters({
	filter,
	onFilterChange,
	candidateTotal,
	running,
	starting,
	onStart,
	onCancel,
}: SpeedTestFiltersProps) {
	const { t } = useTranslation();
	const { data: providers } = useProviders();
	const { data: virtualModels } = useVirtualModels();

	const providerOptions = useMemo<MultiSelectOption[]>(
		() =>
			(providers ?? [])
				.filter((provider) => provider.enable)
				.map((provider) => ({ value: String(provider.id), label: provider.name })),
		[providers],
	);

	const vmOptions = useMemo<MultiSelectOption[]>(
		() =>
			(virtualModels ?? []).map((vm) => ({
				value: String(vm.virtualModelId),
				label: vm.displayId,
			})),
		[virtualModels],
	);

	const disabled = running || starting;

	return (
		<Card>
			<CardContent className="flex flex-wrap items-center gap-2 pt-6">
				<MultiSelect
					options={providerOptions}
					selected={filter.providerIds.map(String)}
					onChange={(values) => onFilterChange({ ...filter, providerIds: values.map(Number) })}
					className="w-[160px]"
					aria-label={t("speedTest.filterByProvider")}
				/>
				<MultiSelect
					options={vmOptions}
					selected={filter.vmIds.map(String)}
					onChange={(values) => onFilterChange({ ...filter, vmIds: values.map(Number) })}
					className="w-[180px]"
					aria-label={t("speedTest.filterByVirtualModel")}
				/>
				<div className="w-[220px]">
					<SearchInput
						value={filter.keyword}
						onChange={(keyword) => onFilterChange({ ...filter, keyword })}
						placeholder={t("speedTest.keywordPlaceholder")}
						aria-label={t("speedTest.keywordPlaceholder")}
					/>
				</div>

				<div className="ml-auto flex items-center gap-2">
					<span className="text-sm text-muted-foreground">
						{candidateTotal === undefined
							? t("speedTest.calculating")
							: t("speedTest.candidateCount", { count: candidateTotal })}
					</span>
					{running ? (
						<Button variant="outline" onClick={onCancel}>
							<Square className="size-4" />
							{t("speedTest.cancel")}
						</Button>
					) : (
						<Button onClick={onStart} disabled={disabled || candidateTotal === 0}>
							{starting ? (
								<Loader2 className="size-4 animate-spin" />
							) : (
								<Gauge className="size-4" />
							)}
							{t("speedTest.start")}
						</Button>
					)}
				</div>
			</CardContent>
		</Card>
	);
}
