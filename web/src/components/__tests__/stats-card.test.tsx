import { StatsCard } from "@/components/stats-card";
import { render, screen } from "@testing-library/react";
import { Clock } from "lucide-react";
import { describe, expect, it } from "vitest";

describe("StatsCard", () => {
	it("renders label, value and subLabel", () => {
		render(<StatsCard icon={Clock} label="总任务" value={12} subLabel="全部任务" />);
		expect(screen.getByText("总任务")).toBeInTheDocument();
		expect(screen.getByText("12")).toBeInTheDocument();
		expect(screen.getByText("全部任务")).toBeInTheDocument();
	});
});
