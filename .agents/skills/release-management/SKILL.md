---
name: release-management
description: 发布 llm-gateway 的生产/开源/Release 版本——同步修改 Cargo.toml 与 web/package.json 版本号、打 v* tag、push 触发 Release CI 发布版本化 ghcr 镜像与 GitHub Release 页。当用户说「发布版本」「发 vX.Y.Z」「出个 Release」时使用；FRP 本地部署（.deploy/deploy.sh）不属于本 skill。
---

# Release Management

llm-gateway 的正式发布走 **GitHub Release** 渠道，与 FRP/阿里云本地部署完全分离。本 skill 描述从「确定版本号」到「Release 页上线」的完整流程。

## When to use

用户明确表示要发布生产/开源/Release 版本，例如「发布一个版本」「发 v1.2.0」「出 Release」。此时需要**同时修改 `Cargo.toml` 与 `web/package.json` 两个版本号**。

**When not to use**：本地部署到 FRP 阿里服务器（`gateway.ijkzen.cn`）走 `.deploy/deploy.sh`（zig 交叉编译 + compose 重建），镜像 tag 固定 `llm-gateway:latest`，**不读不写版本号**，不要混入发布流程。

## 版本号权威与同步点

| 位置 | 角色 | 发版时动作 |
| --- | --- | --- |
| `Cargo.toml` `version` | 权威，后端编译期注入 | **必须修改**，然后跑 `cargo check` 同步 `Cargo.lock` |
| `web/package.json` `version` | 权威 | **必须修改**，与 Cargo.toml 保持一致 |
| `AGENTS.md` 项目概述「版本」行 | 手动维护 | 顺手同步 |
| `README.md` Docker 运行章节的固定版本示例（如 `:v0.1.0`） | 手动维护 | 顺手同步 |
| 前端侧边栏 UI 版本显示 | 自动（读 `/api/healthz` 的 `version` 字段） | **无需修改** |
| `src/usage/http.rs` User-Agent | 自动（`CARGO_PKG_VERSION`） | 无需修改 |

校验脚本：`bash scripts/check-release-version.sh X.Y.Z`（不带 `v`），比对两个权威文件与入参是否一致，本地预检与 Release CI 复用。

## 发布流程

1. **确定版本号**：用户指定，或从上一版按 semver 递增（`X.Y.Z`）。
2. **同步修改两个权威版本文件**：`Cargo.toml` 的 `version = "X.Y.Z"` 与 `web/package.json` 的 `"version": "X.Y.Z"`，两者必须一致；改完 Cargo.toml 后执行 `cargo check` 让 `Cargo.lock` 同步。
3. **同步手动维护点**：`AGENTS.md` 版本行、`README.md` 的固定版本镜像示例。
4. **本地预检**：`bash scripts/check-release-version.sh X.Y.Z`，通过后继续。
5. **全量质量门**（对齐 AGENTS.md 提交约定，必须全绿）：
   ```bash
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets
   cd web && pnpm lint && pnpm vitest run
   ```
6. **提交并推送 main**：commit 消息如 `chore: release vX.Y.Z`；push main 会触发 `ci.yml`（测试）与 `nightly.yml`（构建 `:nightly`），与发布无关，可并行。
7. **打 tag 并推送**：`git tag vX.Y.Z && git push origin vX.Y.Z`。push tag 触发 `release.yml`，自动完成：
   - 版本一致性校验（脚本，不一致即失败）
   - `cargo test --all-targets` + `pnpm vitest run`
   - 构建推送 `ghcr.io/ijkzen/llm-gateway:vX.Y.Z`（同时更新 `:latest`）
   - `softprops/action-gh-release` 创建 GitHub Release 页（自动生成 changelog）
8. **人工检查**：到 GitHub Releases 页核对自动 changelog，补充发布说明后发布。

## 失败与补救

- **版本校验失败**（CI 报 version 不一致）：说明改完版本后没同步两个文件，或 tag 与文件不一致。修正文件并提交，然后 `git tag -d vX.Y.Z`、`git push origin :refs/tags/vX.Y.Z` 删除远端 tag，重新打 tag 推送。
- **测试/构建失败**：修复代码并提交后，重打 tag（同上）。已推的 `:vX.Y.Z` 镜像与 Release 页可覆盖重建（同一 tag 再次构建推送会覆盖）。
- **打错 tag**：删除 tag 重新打即可，Release 页未创建则无残留。

## 渠道与 tag 矩阵

| 渠道 | 触发 | 镜像 tag | 说明 |
| --- | --- | --- | --- |
| `ci.yml` | push main / PR | 无 | 测试、clippy、fmt |
| `nightly.yml` | 每次 push main | `ghcr.io/ijkzen/llm-gateway:nightly`（覆盖式） | 尝鲜，不建 Release，不可作生产依赖 |
| `release.yml` | push `v*` tag | `:vX.Y.Z` + `:latest` | 正式发布，含 GitHub Release 页 |

README 快速开始引用 `:latest`（最新正式版）；生产建议固定 `:vX.Y.Z`；尝鲜用 `:nightly`。
