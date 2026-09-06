# 05: 供应商模型表单三块知识归一

**What to build:** 供应商模型的字段知识只剩一份：校验 schema（上下文长度/最大输出 + 校验文案）、四个能力开关网格、proxy 校验规则（与 proxy 配置字段组件同址导出）。添加与详情弹窗改用共享实现，弹窗各自的打开/重置/toast 骨架不动。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [x] schema 直测：必填/正整数校验、values→payload 映射，不渲染弹窗
- [x] proxy 校验规则与 proxy 字段组件同址，添加/详情两处引用同一份
- [x] 能力开关网格一份实现，新增能力开关只改一处
- [x] 弹窗交互测试瘦身后关键路径仍绿（含幽灵提交守卫回归）
- [x] pnpm lint + vitest 全绿

## Comments

- c45e12a（feat/frontend-panel-convergence）实施完成。provider-model-form 模块：positiveIntField/makeProviderModelBaseSchema + CapabilitySwitchGrid（复用 CapabilityIcons 的 CAPABILITIES，删除重复 CAPABILITY_KEYS）+ proxySuperRefine 与 ProxyConfigFields 同址；schema/proxy 规则直测 6 例。
