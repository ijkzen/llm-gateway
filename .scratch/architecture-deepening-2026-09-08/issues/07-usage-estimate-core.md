# 07: 用量预估纯核心——bug 高发区获得单测面（C5）

**What to build:** `get_provider_usage_estimate`（routes/providers.rs 845-984，140 行内联；三次修复 645bca1/d807c27/6b77d96 含一次回归全住此区）拆成：纯核心 `estimate()`（入参=窗口起点、按日用量序列、配额比例——无 SQL 依赖）+ 显式信任谓词（固化 645bca1 后的现状语义：`estimatable = used > 0 ∧ ratio.is_some()`，无按天闸门）+ 薄 adapter（SQL 窗口起点重建 923-930 留在 handler）。「数据完整吗」与「怎么折算」从此是代码里可指名、可单测、可另行拍板的两问（按天覆盖闸门恢复与否是独立产品讨论，不在本票改变行为）。纯函数住进用量域（如 src/usage/ 新模块或 types 旁，以依赖方向定），handler 只做 SQL 装配。实施时在 CONTEXT.md 用量域补「用量预估 (Usage Estimate)」术语。

**Blocked by:** None（providers.rs 区域独立，可并行）。

**Status:** ready-for-agent

- [x] 纯核心单测：配额比例折算、周×4 折月、整天分桶、used=0/ratio=None 边界（把 usage_estimate_integration 五场景的算术部分下沉为直测单元）
- [x] 现状语义固化：used=0 / ratio 缺失 / limit=0 / unavailable 等边界与 645bca1 部署后逐项一致
- [x] handler 只留 SQL 窗口重建 + 纯函数装配，routes/providers.rs 净减可度量
- [x] usage_estimate 集成场景全绿；全量质量门绿

## Comments

## Comments

- feat/usage-estimate-core 实施完成（质量门全绿：cargo test 796 / 31 套件 + usage_estimate 集成 5 场景全绿，clippy 零警告，fmt 干净；FE 未动）。提交留在分支未合 main。
- **落地形态**：新建 `src/usage/estimate.rs` 纯核心（无 SQL/DB 依赖）：`period_len_ms`（周 7 天/月 30 天）、`quota_ratio`（used/limit 优先、used_percent 兜底、非正与不可折算过滤）、`is_estimatable`（信任边界显式化：used>0 ∧ 比例可折算——645bca1 后现状语义固化，按天覆盖检查不参与算术）、`estimated_total`（比例折算 round）。handler 删内联折算算术与 WEEK/MONTH 常量，只留窗口选取/SQL 统计/响应装配。单测 4 个覆盖：used/limit 与 percent 优先级、limit=0/unavailable/used=0 边界、period 长度映射、估算取整与信任门。CONTEXT.md 用量域补「用量预估 (Usage Estimate)」术语。
- 双轴评审未单独跑（07 为同批小重构，集成 5 场景逐断言兜底；01-06 均跑过双轴）。遗留记录：`estimated_total` 本身对 used=0 返回 Some(0)（不自守门），由 handler 的 estimatable 分支负责输出 None——单测已把该组合固定下来防止误"修复"。