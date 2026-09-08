# 07: 用量预估纯核心——bug 高发区获得单测面（C5）

**What to build:** `get_provider_usage_estimate`（routes/providers.rs 845-984，140 行内联；三次修复 645bca1/d807c27/6b77d96 含一次回归全住此区）拆成：纯核心 `estimate()`（入参=窗口起点、按日用量序列、配额比例——无 SQL 依赖）+ 显式信任谓词（固化 645bca1 后的现状语义：`estimatable = used > 0 ∧ ratio.is_some()`，无按天闸门）+ 薄 adapter（SQL 窗口起点重建 923-930 留在 handler）。「数据完整吗」与「怎么折算」从此是代码里可指名、可单测、可另行拍板的两问（按天覆盖闸门恢复与否是独立产品讨论，不在本票改变行为）。纯函数住进用量域（如 src/usage/ 新模块或 types 旁，以依赖方向定），handler 只做 SQL 装配。实施时在 CONTEXT.md 用量域补「用量预估 (Usage Estimate)」术语。

**Blocked by:** None（providers.rs 区域独立，可并行）。

**Status:** ready-for-agent

- [ ] 纯核心单测：配额比例折算、周×4 折月、整天分桶、used=0/ratio=None 边界（把 usage_estimate_integration 五场景的算术部分下沉为直测单元）
- [ ] 现状语义固化：无 startTime/窗口不足一天、闲置日等场景行为与 645bca1 部署后逐项一致
- [ ] handler 只留 SQL 窗口重建 + 纯函数装配，routes/providers.rs 净减可度量
- [ ] usage_estimate 集成场景全绿；全量质量门绿

## Comments
