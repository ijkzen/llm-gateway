---
name: merge-branches
description: 把本地已完成的特性/修复分支逐个合入 main 并清理。当用户说「把没合并的分支都合进 main」「检查还有哪些分支没合并」「合并完跑质量门」「把已合并的分支删掉」「合一下 XX 分支」或要求批量合并本地分支时使用；流程 = 盘点（区分已合并/未合并）→ 逐个 --no-ff 合入 main → 解决冲突 → 全量质量门全绿 → 删除已合并分支与其 worktree。已合并分支不重复合并。注意：本仓库使用 git worktree 管理功能分支，分支常挂在独立 worktree 上，删除前必须先用 git worktree remove 移除。
---

# 批量合并本地分支到 main（merge-branches）

把本地尚未合并进 `main` 的特性/修复分支依次 `--no-ff` 合入，解决冲突，跑完整质量门到全绿，最后清理已合并分支及其 worktree。

## When to use

用户提到合并本地分支 / 检查未合并分支 / 批量合入 / 合完跑质量门 / 删除已合并分支等意图时使用。典型触发语：

- 「把还没合到 main 的分支都合进来」
- 「检查一下有哪些分支没合并，依次合入」
- 「合并完把已合并的分支删掉」
- 「合一下 fix/xxx / feat/xxx」

**When not to use**：单个小改动直接用 `git merge` 的场景不需要本 skill 全流程；若用户只要求合并**某一个**明确分支（非批量盘点），可走本 skill 的合并+质量门+清理子流程，跳过盘点。

## 背景：本仓库的分支组织方式（必须先理解）

本仓库按惯例用 **git worktree** 开发功能分支：每个分支一个独立目录，如
`../llm-gateway-<slug>/`（与仓库根目录同级）。后果：

- 分支往往** checkout 在一个 worktree 上**，直接用 `git branch -d` 删除会被 git 拒绝（"checked out at ..."）。
- 删除分支前必须先在主仓库执行 `git worktree remove <路径>`，再 `git branch -d`。
- `git branch --merged` / `git branch --no-merged` 仍能正确区分已合并分支。

先跑 `git worktree list` 确认每个分支的挂载目录与干净状态，再动手。

## 流程总览

1. **盘点**：区分已合并 / 未合并分支，并核对 worktree。
2. **合并**：逐个把未合并分支 `--no-ff` 合入 main（含冲突解决）。
3. **质量门**：跑完整质量门，不通过则修复、提交、重跑，直到全绿。
4. **清理**：删除已合并分支及其 worktree。
5. **收尾**：汇报结果，推送前先询问。

## 详细步骤

### 第 1 步：盘点

```bash
git branch -a                      # 全部本地+远程分支
git branch --merged main           # 已合并到 main 的分支（含 main 自己）
git branch --no-merged main        # 尚未合并的分支（真正需要合入的）
git worktree list                  # 分支 → worktree 目录映射
```

判定规则：

- **`git branch --merged main` 列出的分支 = 已合并，跳过，绝不重复合并**（包括 `main` 自身，勿删）。
- **`--no-merged` 列出的才是要合入的候选**。注意：本地分支若已 push 且远端对应分支已合入，本地仍可能显示 no-merged（以本地 main 为准判断即可，不重复合并已含提交的分支）。
- 若一个分支 `--merged` 且没有自己独有的提交（`git log main..分支` 为空），说明其提交已在 main 中，直接跳过。

每个待合分支确认改动范围，预判冲突：

```bash
git log --oneline main..<分支>      # 分支独有提交
git diff --stat main...<分支>       # 改动文件清单
```

与本次已合/将合分支、以及 main 近况对比，重叠文件多的容易冲突。**spec/issue 文档（`.scratch/` 下）与代码可能同时被多分支改动**，注意观察。

### 第 2 步：逐个合并

按依赖/规模排序（一般从小到大；若有明确依赖关系先合被依赖的），**一次一个**地合：

```bash
git merge --no-ff <分支> -m "merge: <分支> into main"
```

- 全部使用 `--no-ff`，保留「分支已合入」的合并提交痕迹。
- **每合完一个立即看结果**，不要一口气合多个再处理。
- 合并过程发生冲突时，git 会中断并列出冲突文件（`git status` 可见 `UU`），此时按下方「冲突解决」处理，全部解决完 `git add` 后再继续 `git commit` 完成这次合并。

### 第 3 步：质量门（合并全部完成后）

所有分支合完后，按 AGENTS.md「提交前置动作」跑完整质量门，**必须全绿**：

```bash
cargo fmt                          # 后端全库格式化（会改动文件）
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cd web && pnpm lint                # biome check .
cd web && pnpm vitest run
```

（后端测试较慢；pnpm vitest run 可放后台跑以节省时间，但必须等其退出并核对输出。）

- **fmt 后 git diff 检查**：`cargo fmt` 可能改动代码，确认无异常。
- 质量门任一失败 → 修复 → 提交（`fix: ...` 或随合并一并）→ 重跑**完整**质量门，直到全绿。禁止带警告/失败提交。
- 参考：`references/quality-gate.md`（各步骤要点与常见失败）。

### 第 4 步：清理

质量门全绿后，把**所有已合并分支**（含本次合并的 + 之前就已合并但未清理的）删除：

```bash
# 1) 先移除每个已合并分支的 worktree（目录须干净，否则会被拒）
git worktree remove <worktree路径>
# 2) 再删除分支（-d 安全删除，git 会校验已合并）
git branch -d <分支名> ...
```

边界与禁忌：

- **`git branch -d` 拒绝未合并分支**（提示分支未完全合并）：不要用 `-D` 强删！先确认它是否真的已合并。若确实已合并但仍被拒，通常是**该分支还 checkout 在某个 worktree 上**——先 `git worktree remove` 那个 worktree 再删。
- **worktree remove 前必须检查该目录 git 状态干净**（有未提交改动会被拒；真有需要保留的改动先处理，别丢数据）。
- **`main` 与当前所在分支绝不删除**。删除前 `git status` 确认当前在 main。
- **推送前询问**：删除分支不影响已推送的远端分支（如想删远端：`git push origin --delete <分支>`，需用户确认）。
- 收尾用 `git branch` + `git worktree list` 确认只剩 main 与主 worktree。

### 第 5 步：收尾汇报

汇报格式：盘点了哪些分支 → 哪些已合并跳过 → 哪些本次合入（合并提交 hash）→ 是否遇到冲突及如何解决 → 质量门结果（逐项）→ 清理结果。**推送（git push）前先询问用户**。

## 冲突解决

### 通用流程

1. `git status` 看冲突文件（`UU`/`AA`/`DD`/`AU`/`UA`），逐文件解决。
2. 打开每个冲突文件，git 标记 `<<<<<<< / ======= / >>>>>>>` 标明两侧来源：
   - `HEAD` 侧 = main 当前内容；`<分支>` 侧 = 待合分支内容。
3. 逐个冲突块判断保留哪侧 / 两侧合并 / 重新实现，**解决后删掉标记行**。
4. 同类文件可能有多处冲突，全部处理完，`git add <文件>` 标记已解决。
5. 全部冲突解决并 add 后执行 `git commit`（使用 merge 预填的提交信息），完成本次合并。
6. **验证先行**：冲突解决后跑针对性测试（如 `cargo test` 相关模块 / `pnpm vitest run` 相关测试文件），确认合并产物行为正确——"能编译"不等于"语义正确"。

### 关键原则：规格优先、不擅自改语义

**遇到无法一眼判断 / 两侧语义冲突 / 合并结果存疑的冲突时：**

1. **先查规格文件**：本仓库需求与规格在 `.scratch/<feature-slug>/` 下，通常含 `REQUIREMENTS.md`、`SPEC.md` 与 `issues/NN-*.md`（NN=数字前缀）。`ls .scratch/` 列出全部 feature slug，按分支名（如 `feat/api-key-race-nav` → `.scratch/api-key-race-nav/`）找对应规格。规格明确则以规格为准解决，冲突解决后 **commit message 或汇报中注明依据**。
2. **规格存在但不足以定夺 / 规格之间互相矛盾 / 无对应规格** → **不要猜**，用 AskUserQuestion 提问。把问题讲清楚：冲突的文件与行、两侧各自想表达什么（结合代码与对应规格）、可能的影响。**这是用户明确要求的行为边界——不可自行发明解决方案。**
3. 两侧改动**目标一致且可共存**（如同一个 i18n 文件各加不同 key、同一个文件不同段落各加功能、一个改了函数签名另一个改了调用点需要适配）时，按「保留双方语义、互补合并」处理。
4. 明显偏向一侧（如一侧只是重构/格式，另一侧是新功能）时，保功能侧并融入另一侧有效改动。
5. **不要默默丢弃任一分支的实质性改动**——尤其当一侧是修复、另一侧是特性时，两者都要保留。
6. 涉及「口径/行为定义」（如统计窗口、字段含义、状态机）的冲突，对照 `.scratch/<slug>/SPEC.md` 与 `issues/` 逐条核对，避免合出与规格相悖的行为。

### 本仓库常见冲突场景与对策

| 场景 | 对策 |
| --- | --- |
| 两个分支改同一个 i18n 文件（en.ts/zh-CN.ts）不同 key | 双方 key 都保留（互补合并），注意中英双语 key 必须成对出现 |
| 两个分支改同一个测试文件不同用例 | 按 describe/test 块重组，保留双方用例；git 自动合并出错时从父 blob 手工合并 |
| 一个分支改函数签名，另一分支改了该函数调用点 | 保留新签名，适配调用点，两端语义都要对 |
| 后端 `src/routes/*.rs` / 实体同文件各加各的 | 通常可自动合并；手动冲突时按区块互补 |
| 前端组件同文件（如 overview 页同时加两排卡片） | 布局/顺序冲突需结合 SPEC 判断；两功能目标不同通常并存 |
| 一方是 `docs:` 提交（.scratch/ 规格） | 规格文档按内容合并，新增各自文件即可；同名文件冲突按内容取舍 |
| 迁移版本号冲突（db.rs 增量迁移版本号） | ⚠️ 生产库可能残留旧迁移记录，撞号会静默吞掉 ALTER——合并若改迁移号，先对照记忆与现有 schema_migrations 再定号，部署后需验证列存在 |

**深入参考**：`references/conflict-resolution.md` 含规格查找、AskUserQuestion 提问模板与更多仓库特定冲突的处理细节。

## 常见错误与边界情况速查

| 情况 | 正确处理 |
| --- | --- |
| 分支已合并但 `git branch -d` 拒绝 | 分支 checkout 在 worktree 上 → 先 `git worktree remove` 该目录再 `-d` |
| 分支未合并却想删 | 用 `-d` 会被拒（保护）；确实要合先合，别用 `-D` 绕过 |
| 本地分支很多，不知哪些要合 | 以 `git branch --no-merged main` 为准，`--merged` 的一律跳过 |
| 合并冲突无法判断 | 查 `.scratch/<slug>/` 规格；仍不明确 → AskUserQuestion 提问，不猜 |
| 无规格文件 | 不擅自决定，AskUserQuestion 讲清两侧内容让用户拍板 |
| 合并到一半想放弃 | `git merge --abort` 回到合并前（未提交时可用） |
| 自动合并成功但语义可疑 | 跑相关测试验证；不过则修复 |
| fmt 改了无关文件 | 正常，cargo fmt 全库格式化；确认无逻辑改动即可 |
| 质量门不通过 | 修复→提交→重跑完整质量门，直到全绿，不跳过不粉饰 |
| main 领先 origin/main 很多未推送 | 合并/清理不自动 push；收尾询问用户是否推送 |
| 分支同时改了 .scratch 规格与代码 | 规格文档是提交内容（docs:），冲突时按内容合并，不删任一方 |
| worktree 目录不干净 | 先处理（提交/暂存/确认丢弃），不强行 remove |

## 注意事项

- 本仓库特性分支常挂在独立 worktree 上（同级 `../llm-gateway-<slug>/` 目录），**合并前不需要、也不建议去 worktree 里操作**，直接在 main 仓库合即可。
- 合并提交信息统一 `merge: <分支> into main`。
- 全程保持当前在 main 分支、工作区尽量干净再开始。
- 规格/issue 目录 `.scratch/` 下的文档是 git 跟踪的正式内容，合并后一并保留。
- 若本地 main 落后于远端（`git status` 显示 behind），先 `git pull`（或确认要基于哪个 base 合并）再开始。
