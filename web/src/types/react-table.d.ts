import "@tanstack/react-table";

declare module "@tanstack/react-table" {
	// 允许在列定义中通过 meta.title 声明中文列名，供列显隐菜单展示
	interface ColumnMeta<TData, TValue> {
		title?: string;
	}
}
