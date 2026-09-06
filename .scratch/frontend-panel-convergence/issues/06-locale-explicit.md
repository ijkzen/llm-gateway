# 06: 纯格式化函数 locale 显式化

**What to build:** 窗口周期/分桶标签、负载均衡策略标签等纯格式化函数通过显式 locale 形参取语言，组件在边缘经现有翻译 hook 传入；函数本体不再读 i18n 全局单例，en 格式化分支首次可直测。API 客户端错误文案不在范围内。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [x] 每个改写的纯函数有 zh 与 en 两个直测分支（en 例：月份桶 → "Aug 2026"）
- [x] 调用点显式传 locale，函数内无全局单例读取
- [x] 组件层展示输出与升级前一致
- [x] pnpm lint + vitest 全绿

## Comments

- 96fa9cb（feat/frontend-panel-convergence）实施完成。formatTokenCount/formatReadableNumber/formatPeriodLabel/formatBucketLabel 加显式 locale 形参（Locale/localeOf 入 lib/utils）；loadBalancingLabel/fallbackLabel/otherLabel 注入 t；lib 层零全局 i18n 读取；en 分支直测（Aug 2026 / 1.5K 等）。api.ts 不动。

- 评审整改：补 formatReadableNumber zh/en 直测；说明 otherLabel/loadBalancingLabel/fallbackLabel 为 t 注入的薄包装（单测价值低，行为由调用方 contract 覆盖）。
