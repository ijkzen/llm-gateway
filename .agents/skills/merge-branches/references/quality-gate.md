# 质量门要点（merge-branches 用）

质量门命令与顺序以 AGENTS.md「提交前置动作」与仓库 CI 为准，这里是执行细节与常见失败对照。**目标是全绿，任一失败都要修复后重跑完整质量门，直到全绿才可提交。**

## 命令与顺序

```bash
cargo fmt                          # 后端全库格式化（非局部）
cargo clippy --all-targets --all-features -- -D warnings   # 零警告
cargo test --all-targets           # 后端全量测试
cd web && pnpm lint                # 前端 biome check .（tab 缩进/双引号/100 列）
cd web && pnpm vitest run          # 前端全量测试（pnpm test 是 watch 模式，不要用）
```

tips：

- 前端测试（vitest）独立于后端，可放后台跑以并行节省时间，但**必须等它退出并读完整输出**（45 个测试文件、约 300+ 用例，正常十几秒内完成）。
- `cargo test --all-targets` 跑得最久（单测 + 十几个集成测试文件），timeout 给足（600000ms）。
- fmt 是第 1 步：clippy/test 之前先格式化，避免 clippy 报格式类问题浪费一轮。

## 各步骤常见失败与修复

### cargo fmt
- 正常会改动少量文件（合并后代码格式漂移）。`git diff` 看一眼确认只有格式改动即可。
- 不会"失败"，除非有语法错误。

### cargo clippy -D warnings
- 合并产生的 dead code / unused import（例如一侧删了函数、另一侧还引用）最常见。
- 还有语义冲突导致的类型不匹配（改签名没适配调用点）也会在这里暴露。
- 修复后重跑 clippy 确认零警告。

### cargo test --all-targets
- 合并冲突解决得不对 → 编译错误或测试失败，会直接暴露。
- **语义合并错误**：测试可能失败（如统计口径变了、排序规则变了）。失败测试是"两侧语义没合对"的重要信号，对照 SPEC 修，不要为了过测试而改测试断言（除非断言确实过时）。
- 依赖全局 tracing subscriber 的测试（log_capture、worker 日志链路）串行执行，别慌，正常等待。

### pnpm lint（biome check .）
- 常见：多余的 import、未使用的变量/参数（TS 严格模式 noUnusedLocals）、行超 100 列、引号/缩进不符（tab 缩进 + 双引号）。
- 合并容易带进"两侧各自引入但已不需要"的 import，手动清掉。

### pnpm vitest run
- 前端组件测试可能因合并后 DOM 结构/文案变化失败（测试断言的是 UI 行为）。
- 若两侧都改了同一测试文件且 git 自动合出错，会出现"缺 helper 引用 / import 缺失 / 旧结构断言"这类语义错配——按 describe 块从两侧重组，必要时看两侧父 blob：
  ```bash
  git show <main侧sha>:<文件>     # HEAD 侧原样
  git show <分支sha>:<文件>        # 分支侧原样
  ```
- 用 `pnpm vitest run <单文件>` 快速迭代单个失败文件。

## 修复后重跑

修复代码 → 若修复产生新改动需 `git commit`（可以随合并提交一起，也可以单独 `fix:` 提交）→ **重新跑完整质量门五项**，直到全部通过。不要只重跑失败的那一项就宣布完成——某次修复可能引入新问题。
