# 06: 纯格式化函数 locale 显式化

**What to build:** 窗口周期/分桶标签、负载均衡策略标签等纯格式化函数通过显式 locale 形参取语言，组件在边缘经现有翻译 hook 传入；函数本体不再读 i18n 全局单例，en 格式化分支首次可直测。API 客户端错误文案不在范围内。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] 每个改写的纯函数有 zh 与 en 两个直测分支（en 例：月份桶 → "Aug 2026"）
- [ ] 调用点显式传 locale，函数内无全局单例读取
- [ ] 组件层展示输出与升级前一致
- [ ] pnpm lint + vitest 全绿
