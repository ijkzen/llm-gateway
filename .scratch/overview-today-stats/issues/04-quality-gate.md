# 04: 全量质量门验证

**What to build:** 在独立 worktree 上对完整改动跑一遍仓库提交门禁：cargo fmt、cargo clippy
--all-targets --all-features -D warnings、cargo test --all-targets、web pnpm lint、pnpm vitest run，
全部绿才视为完成（既有代码引起的告警一并处理）。

**Blocked by:** 01、02、03

**Status:** ready-for-agent

- [ ] cargo fmt 无差异
- [ ] clippy 零警告
- [ ] cargo test --all-targets 全绿（含既有集成测试）
- [ ] pnpm lint 全绿
- [ ] pnpm vitest run 全绿
