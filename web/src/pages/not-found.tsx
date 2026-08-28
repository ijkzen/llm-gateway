import { Button } from "@/components/ui/button";
import { useNavigate } from "react-router-dom";

export default function NotFoundPage() {
	const navigate = useNavigate();

	return (
		<div className="flex flex-1 flex-col items-center justify-center gap-2 py-16">
			<h1 className="text-7xl font-bold leading-tight">404</h1>
			<span className="font-medium">哎呀！页面走丢了</span>
			<div className="mt-6 flex gap-4">
				<Button variant="outline" onClick={() => navigate(-1)}>
					返回上页
				</Button>
				<Button onClick={() => navigate("/")}>返回首页</Button>
			</div>
		</div>
	);
}
