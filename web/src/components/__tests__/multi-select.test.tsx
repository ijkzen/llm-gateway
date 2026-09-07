import { MultiSelect, type MultiSelectOption } from "@/components/multi-select";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const options: MultiSelectOption[] = [
	{ value: "a", label: "Alpha" },
	{ value: "b", label: "Beta" },
	{ value: "c", label: "Gamma" },
];

function setup(selected: string[]) {
	const onChange = vi.fn();
	render(
		<MultiSelect options={options} selected={selected} onChange={onChange} aria-label="测试多选" />,
	);
	return onChange;
}

function openPopover() {
	fireEvent.click(screen.getByRole("button", { name: "测试多选" }));
}

describe("MultiSelect", () => {
	it("未选择时触发器显示「全部」", () => {
		setup([]);
		expect(screen.getByRole("button", { name: "测试多选" })).toHaveTextContent("全部");
	});

	it("单选时触发器显示选项名", () => {
		setup(["a"]);
		expect(screen.getByRole("button", { name: "测试多选" })).toHaveTextContent("Alpha");
	});

	it("多选时触发器显示已选数量", () => {
		setup(["a", "b"]);
		expect(screen.getByRole("button", { name: "测试多选" })).toHaveTextContent("已选 2 项");
	});

	it("勾选未选项回调新增后的集合", () => {
		const onChange = setup(["b"]);
		openPopover();
		fireEvent.click(screen.getByText("Alpha"));
		expect(onChange).toHaveBeenCalledWith(["a", "b"]);
	});

	it("取消已选项回调剩余集合", () => {
		const onChange = setup(["a", "b"]);
		openPopover();
		fireEvent.click(screen.getByText("Beta"));
		expect(onChange).toHaveBeenCalledWith(["a"]);
	});

	it("手动勾满所有选项时归一化为「全部」（回调空数组）", () => {
		// 「全部」态下取消 Alpha 再勾回 → 全满 → 空数组。
		const onChange = setup(["b", "c"]);
		openPopover();
		fireEvent.click(screen.getByText("Alpha"));
		expect(onChange).toHaveBeenCalledWith([]);
	});

	it("「全部」态下点击子条目退化为仅选该项", () => {
		const onChange = setup([]);
		openPopover();
		fireEvent.click(screen.getByText("Beta"));
		expect(onChange).toHaveBeenCalledWith(["b"]);
	});

	it("「全部」态下子条目显示为未勾选，仅「全选」为勾选态", () => {
		setup([]);
		openPopover();
		expect(screen.getByRole("checkbox", { name: "全选" })).toBeChecked();
		expect(screen.getByRole("checkbox", { name: "Alpha" })).not.toBeChecked();
		expect(screen.getByRole("checkbox", { name: "Beta" })).not.toBeChecked();
		expect(screen.getByRole("checkbox", { name: "Gamma" })).not.toBeChecked();
	});

	it("显式选中时子条目按选择勾选，全选为半选态", () => {
		setup(["a", "c"]);
		openPopover();
		expect(screen.getByRole("checkbox", { name: "全选" })).not.toBeChecked();
		expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeChecked();
		expect(screen.getByRole("checkbox", { name: "Beta" })).not.toBeChecked();
		expect(screen.getByRole("checkbox", { name: "Gamma" })).toBeChecked();
	});

	it("搜索框按关键词过滤选项，无匹配显示提示", () => {
		setup([]);
		openPopover();
		fireEvent.change(screen.getByPlaceholderText("搜索选项…"), {
			target: { value: "gam" },
		});
		expect(screen.getByText("Gamma")).toBeInTheDocument();
		expect(screen.queryByText("Alpha")).not.toBeInTheDocument();
		expect(screen.queryByText("Beta")).not.toBeInTheDocument();

		fireEvent.change(screen.getByPlaceholderText("搜索选项…"), {
			target: { value: "zzz" },
		});
		expect(screen.getByText("无匹配选项")).toBeInTheDocument();
	});

	it("全选行点击后归一化为「全部」（回调空数组）", () => {
		const onChange = setup(["a"]);
		openPopover();
		fireEvent.click(screen.getByLabelText("全选"));
		expect(onChange).toHaveBeenCalledWith([]);
	});
});

describe("MultiSelect 分组折叠", () => {
	const grouped: MultiSelectOption[] = [
		{ value: "p1-a", label: "A-1", group: "Provider One" },
		{ value: "p1-b", label: "B-1", group: "Provider One" },
		{ value: "p2-c", label: "C-2", group: "Provider Two" },
	];

	function open() {
		fireEvent.click(screen.getByRole("button", { name: "测试分组多选" }));
	}

	it("带 group 选项：分组标题为可点按钮，默认全展开", () => {
		const onChange = vi.fn();
		render(
			<MultiSelect options={grouped} selected={[]} onChange={onChange} aria-label="测试分组多选" />,
		);
		open();
		// 标题按钮（非 checkbox 的「全选」按钮）
		const g1 = screen.getByRole("button", { name: /Provider One/ });
		const g2 = screen.getByRole("button", { name: /Provider Two/ });
		expect(g1).toHaveAttribute("aria-expanded", "true");
		expect(g2).toHaveAttribute("aria-expanded", "true");
		// 默认全展开：组内选项可见
		expect(screen.getByText("A-1")).toBeInTheDocument();
		expect(screen.getByText("C-2")).toBeInTheDocument();
	});

	it("点击分组标题折叠后组内选项消失，再点展开恢复", () => {
		const onChange = vi.fn();
		render(
			<MultiSelect options={grouped} selected={[]} onChange={onChange} aria-label="测试分组多选" />,
		);
		open();
		fireEvent.click(screen.getByRole("button", { name: /Provider One/ }));
		expect(screen.getByRole("button", { name: /Provider One/ })).toHaveAttribute(
			"aria-expanded",
			"false",
		);
		expect(screen.queryByText("A-1")).not.toBeInTheDocument();
		expect(screen.queryByText("B-1")).not.toBeInTheDocument();
		// 另一组不受影响
		expect(screen.getByText("C-2")).toBeInTheDocument();
		// 再点展开
		fireEvent.click(screen.getByRole("button", { name: /Provider One/ }));
		expect(screen.getByText("A-1")).toBeInTheDocument();
	});

	it("无 group 的选项不渲染分组标题按钮", () => {
		setup([]);
		openPopover();
		expect(screen.queryByRole("button", { name: /Provider/ })).not.toBeInTheDocument();
		// 平铺选项仍在
		expect(screen.getByText("Alpha")).toBeInTheDocument();
	});

	it("折叠分组不影响「全选」行与其它组勾选", () => {
		const onChange = vi.fn();
		render(
			<MultiSelect
				options={[
					{ value: "a", label: "Alpha" },
					{ value: "b", label: "Beta" },
					{ value: "c", label: "Gamma" },
					{ value: "c2", label: "Gamma-2", group: "组二" },
				]}
				selected={["a", "c"]}
				onChange={onChange}
				aria-label="混合多选"
			/>,
		);
		fireEvent.click(screen.getByRole("button", { name: "混合多选" }));
		fireEvent.click(screen.getByRole("button", { name: /组二/ }));
		expect(screen.getByRole("checkbox", { name: "全选" })).toBeInTheDocument();
		expect(screen.queryByText("Gamma-2")).not.toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: /组二/ }));
		fireEvent.click(screen.getByText("Gamma-2"));
		expect(onChange).toHaveBeenCalledWith(["a", "c", "c2"]);
	});
});
