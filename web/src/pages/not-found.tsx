import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

export default function NotFoundPage() {
	const navigate = useNavigate();
	const { t } = useTranslation();

	return (
		<div className="flex flex-1 flex-col items-center justify-center gap-2 py-16">
			<h1 className="text-7xl font-bold leading-tight">404</h1>
			<span className="font-medium">{t("notFound.title")}</span>
			<div className="mt-6 flex gap-4">
				<Button variant="outline" onClick={() => navigate(-1)}>
					{t("notFound.back")}
				</Button>
				<Button onClick={() => navigate("/")}>{t("notFound.home")}</Button>
			</div>
		</div>
	);
}
