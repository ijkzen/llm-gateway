// Intl.supportedValuesOf 的类型补充（TS lib 默认未包含，运行时现代浏览器均支持）。
declare namespace Intl {
	function supportedValuesOf(key: "timeZone"): string[];
}
