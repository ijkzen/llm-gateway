# 08: 全量质量门 + 文档对账

**What to build:** 收尾：cargo fmt/clippy -D warnings/全量测试、前端 pnpm lint + vitest 全绿（前端零改动但门禁照跑）；ADR-0021/CONTEXT 统计域词条与实现现状对账；AGENTS.md 相关段落（表结构/模块/迁移版本/内置任务清单/测试计数）同步；`.scratch/request-log-snapshot/` 票全部置完成态。

**Blocked by:** 01–07 全部

**Status:** ready-for-agent

- [ ] 质量门全绿（后端全量 + 前端全量，不回退既有用例）
- [ ] ADR/CONTEXT/AGENTS 与代码现状一致（docs 改动独立提交）
- [ ] 迁移 26 在全新库与模拟老库（14/15 号段占用）路径均验证
