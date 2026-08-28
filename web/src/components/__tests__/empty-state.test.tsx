import { EmptyState } from "@/components/empty-state";
import { render, screen } from "@testing-library/react";
import { Inbox } from "lucide-react";
import { describe, expect, it } from "vitest";

describe("EmptyState", () => {
	it("renders title, description and action", () => {
		render(
			<EmptyState
				icon={Inbox}
				title="暂无数据"
				description="当前没有任何记录"
				action={<button type="button">新建</button>}
			/>,
		);
		expect(screen.getByText("暂无数据")).toBeInTheDocument();
		expect(screen.getByText("当前没有任何记录")).toBeInTheDocument();
		expect(screen.getByText("新建")).toBeInTheDocument();
	});
});
