import { PageHeader } from "@/components/page-header";
import { SpeedTestPanel } from "@/components/speed-test/SpeedTestPanel";
import { SPEED_TEST_PAGE } from "@/lib/pages";
import { useTranslation } from "react-i18next";

export default function SpeedTestPage() {
	const { t } = useTranslation();
	return (
		<div className="space-y-6">
			<PageHeader icon={SPEED_TEST_PAGE.icon} title={t(SPEED_TEST_PAGE.titleKey)} />
			<SpeedTestPanel />
		</div>
	);
}
