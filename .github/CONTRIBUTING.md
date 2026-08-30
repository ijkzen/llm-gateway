# Contributing to llm-gateway

感谢你对 llm-gateway 感兴趣！这是一个个人维护的项目，欢迎合理的贡献。

## 工作流

本仓库使用基于本地 markdown 文件的 issue 跟踪（`docs/agents/issue-tracker.md`），
目前以 GitHub issue 讨论为主，改动请先提 issue 确认设计再动手。

## 报告问题

如果发现 bug 或希望提出新功能，请先：

1. 阅读 `CONTEXT.md`（领域术语）与 `docs/adr/`（架构决策），确认是否已有相关设计。
2. 描述清楚：期望行为、实际行为、复现步骤、相关日志（`RUST_LOG` 输出）。

安全相关问题**不要**公开报告——见 `SECURITY.md`。

## 本地开发

依赖：Rust（stable，2024 edition）、Node ≥ 22 + pnpm（`corepack enable pnpm`）。

```bash
# 前端构建（rust-embed 编译期嵌入 web/dist，改前端后必须先构建）
cd web && pnpm install --frozen-lockfile && pnpm build && cd ..

# 测试（集成测试会启动真实 app + mock 上游）
cargo test --all-targets

# 代码质量（CI 会做同样的检查）
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
cd web && pnpm vitest run
```

## 提交规范

- 遵循仓库现有代码风格（Rust 2024 edition；前端 Biome tab 缩进、双引号、100 列）。
- 新增功能请同时补充测试（后端 `tests/` 集成测试或模块单元测试；前端 `web/src/__tests__/`）。
- commit message 建议遵循现有惯例（如 `feat(proxy): ...`、`fix(usage): ...`）。

## 联系

通过 GitHub issue 讨论，或邮件联系维护者。
