---
name: release-management
description: 发布 llm-gateway 的生产/开源/Release 版本——同步修改 Cargo.toml 与 web/package.json 版本号、打带版本描述的 annotated tag、push 触发 Release CI 发布版本化 ghcr 镜像与 GitHub Release 页。当用户说「发布版本」「发 vX.Y.Z」「出个 Release」时使用；用户未指定版本号时自动生成（无 tag → 0.1.0，有 tag 则递增，每位上限 99 进位）。FRP 本地部署（.deploy/deploy.sh）不属于本 skill。
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

## 版本号生成规则

用户明确给出 `X.Y.Z` 时直接使用；**未指定时自动生成**：

1. 查询现有 tag：`git tag -l 'v*' --sort=-v:refname`（按版本倒序，取最新的正式 tag）。
2. **无任何 tag** → 默认 `0.1.0`。
3. **已有 tag** → 递增 patch 位：`v0.1.0` → `0.1.1`。**每位上限 99**：`0.1.99` 递增为 `0.2.0`；`0.99.99` 递增为 `1.0.0`（仅 patch 与 minor 参与进位，major 不受 99 限制）。
4. 生成后先向用户展示目标版本号，无异议再继续。

## 版本描述生成规则

每次发版必须打**带版本描述（message）的 annotated tag**：`git tag -a vX.Y.Z -m "<描述>"`。描述**自动生成**，由上一个 tag 到当前提交之间的提交记录总结归纳，**不要**直接贴 git log：

- 先取区间提交：`git log --oneline <上一个tag>..HEAD`（首次发布用 `git log --oneline` 全部提交）。
- 再逐条阅读提交内容（必要时 `git show <sha>` 看 diff），按类别归纳：
  - **新增特性**（feat）：新接口、新页面、新能力，如「新增供应商用量查询」「新增数据面板」。
  - **Bug 修复**（fix）：修了什么、影响什么，如「修复切换语言后查询缓存失效」。
  - **性能提升**（perf）：如「优化分页查询」「连接池复用」。
  - **其他**（chore/docs/style 等）：重构、文档、构建等，按需合并成「其他改进」。
- 同类多条合并成一条，用用户可读的句子概括；描述控制在几行内，中文书写。

## 发布流程

1. **确定版本号**：用户指定则直接用；未指定按「版本号生成规则」自动生成，并先向用户展示。
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
7. **打 annotated tag 并推送**：按「版本描述生成规则」生成描述后，执行
   `git tag -a vX.Y.Z -m "<版本描述>" && git push origin vX.Y.Z`。push tag 触发 `release.yml`，自动完成：
   - 版本一致性校验（脚本，不一致即失败）
   - `cargo test --all-targets` + `pnpm vitest run`
   - 构建推送 `ghcr.io/ijkzen/llm-gateway:vX.Y.Z`（同时更新 `:latest`）
   - `softprops/action-gh-release` 创建 GitHub Release 页（自动生成 changelog；tag 上的版本描述会作为 Release 正文的初始内容）
8. **人工检查**：到 GitHub Releases 页核对自动 changelog，补充发布说明后发布。

## 失败与补救

- **版本校验失败**（CI 报 version 不一致）：说明改完版本后没同步两个文件，或 tag 与文件不一致。修正文件并提交，然后 `git tag -d vX.Y.Z`、`git push origin :refs/tags/vX.Y.Z` 删除远端 tag，重新打 tag 推送。
- **测试/构建失败**：修复代码并提交后，重打 tag（同上）。已推的 `:vX.Y.Z` 镜像与 Release 页可覆盖重建（同一 tag 再次构建推送会覆盖）。
- **打错 tag**：删除 tag 重新打即可（`git tag -d` + `git push origin :refs/tags/...`），Release 页未创建则无残留；若已创建，删除 tag 后 Release 页需在 GitHub 上手动删除或复用新 tag 覆盖。
- **版本描述生成注意**：区间为上一个 tag 到当前 HEAD；若期间有未发布的 tag（如补丁），确保以「最新已发布 tag」为基准，避免重复归纳已发布的提交。

## 渠道与 tag 矩阵

| 渠道 | 触发 | 镜像 tag | 说明 |
| --- | --- | --- | --- |
| `ci.yml` | push main / PR | 无 | 测试、clippy、fmt |
| `nightly.yml` | 每次 push main | `ghcr.io/ijkzen/llm-gateway:nightly`（覆盖式） | 尝鲜，不建 Release，不可作生产依赖 |
| `release.yml` | push `v*` tag | `:vX.Y.Z` + `:latest` | 正式发布，含 GitHub Release 页 |

README 快速开始引用 `:latest`（最新正式版）；生产建议固定 `:vX.Y.Z`；尝鲜用 `:nightly`。
