import { Badge } from "@/components/ui/badge";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import type { CandidateModel } from "@/hooks/use-speed-test";
import { useTranslation } from "react-i18next";

interface SpeedTestCandidatesProps {
	models: CandidateModel[];
}

/**
 * 候选模型预览：筛选条件一变化就列出「将会测这些模型」。
 *
 * 存在的理由：候选集只在点「开始测速」后才变成结果行，否则用户会盯着一个
 * 空页面猜「到底会测谁」。这里与 `speedtest::candidate::resolve` 同源，
 * 展示的就是真正会被发出的请求对象。
 */
export function SpeedTestCandidates({ models }: SpeedTestCandidatesProps) {
	const { t } = useTranslation();
	return (
		<div className="overflow-x-auto">
			<Table>
				<TableHeader>
					<TableRow>
						<TableHead>{t("speedTest.columnProvider")}</TableHead>
						<TableHead>{t("speedTest.columnModel")}</TableHead>
						<TableHead>{t("speedTest.columnVirtualModels")}</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{models.map((model) => (
						<TableRow key={`${model.providerId}-${model.providerModelPk}`}>
							<TableCell className="max-w-[200px]">{model.providerName}</TableCell>
							<TableCell className="max-w-[260px]">{model.modelId}</TableCell>
							<TableCell className="max-w-[240px]">
								{model.virtualModels.length > 0 ? (
									<div className="flex flex-wrap gap-1">
										{model.virtualModels.map((display) => (
											<Badge key={display} variant="secondary">
												{display}
											</Badge>
										))}
									</div>
								) : (
									<span className="text-muted-foreground">{t("speedTest.notInVirtualModel")}</span>
								)}
							</TableCell>
						</TableRow>
					))}
				</TableBody>
			</Table>
		</div>
	);
}
