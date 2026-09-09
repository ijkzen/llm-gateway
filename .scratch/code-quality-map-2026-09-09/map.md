# 全仓代码质量审计地图：模块审查（清单模式，2026-09-09 立项）

## Destination

后端 + 前端全部功能域以模块为单位审毕：每张模块票产出该模块的一份四轴（逻辑正确性 / 实现简洁 / 测试覆盖 / 模块间调用）+ 专门性能与内存轮的分级审查清单（FINDINGS 式，P1-P3 分级、行号锚点逐一与磁盘现状复核），并带归位遗留项的重估结论。**清单模式：本图不实施任何代码改动**；改动在地图走完后统一另行排期。图走完 = 每个模块「改什么 / 为何 / 证据」已成决策文档，无悬而未决的审查问题。

## Notes

- **模式**：审出清单不改（wayfinder plan-only，执行不载入地图）。票内无歧义的问题直接进清单；需要用户拍板的（bug vs 设计如此、重构取舍）走 AskUserQuestion 当场记录结论，仍不改代码。P1 高危发现的修复时机由用户在票答覆盖时自行决定，图内默认不动。
- **审查轴**：四轴 + 每票一轮专门性能/内存审查。2026-09-08 已整改收官的范围不复查（`codebase-audit-2026-09-08/FINDINGS.md` 四维审计、`architecture-deepening-2026-09-08/REVIEW.md` C1-C7、架构评审轮 6 候选）；本票通读若发现新结构使其回归才重记。
- **清单格式**：沿用四维审计 FINDINGS 口径（编号 / 严重度 P1-P3 / 维度 / 一句话 / 证据行号），产出到 `.scratch/code-quality-map-2026-09-09/findings/<NN>-<slug>.md`，Answer 给摘要。行号引用必须逐条复核（教训：子代理报告是起点，执行前逐行核对）。
- **归位遗留项**：S3 request 表保留与 rollup → 03 票；S5 同窗聚合合并 → 10 票；usage fetcher 会话失效分类 → 06 票（均在对应票 Question 中显式列出）。
- **Tracker**：本地 markdown（`docs/agents/issue-tracker.md`）；ticket 均为 `task` 类型（agent 独立驱动），`Status: claimed/resolved` 认领后才动手，Answer 段记录结论。所有模块票被 01 盘点票阻塞。
- **范围权威**：各模块票的审查范围与切分以 `MODULES.md`（01 票产物）§1/§3/§4 为准——01 已解决（2026-09-09），其切分修正（08 补寄居文件、11 补路由文件、18 拆 21、19 缩小、hooks 冻结接口）已直接落进各票 Question。
- **Skills**：与用户拍板用 grilling（AskUserQuestion）；域术语若有冲突用 domain-modeling；无需 research/prototype。

## Decisions so far

- [01 · 模块边界全景盘点](issues/01-module-boundary-inventory.md)：全仓模块图（后端 20 模块两清分层、availability=纯 entity 底座、usage↔proxy 单点双向 by-design）+ 10 族散落候选（F3/F6 已单源免票，F1/F2/F4/F5/F7/F8/F9/F10 各有主票见 MODULES.md §4）+ 文件归属（failure_recovery.rs 寄居成立→08 审、failure_recheck 合理、usage 子模块干净）+ 前端切分修正（18 拆 21、19 缩小、16/17/20 注记）+ AGENTS.md 结构树漂移清单（§1.4，随实施批次刷新）。产物=MODULES.md。
- [02 · proxy 转发编排与选路审查](issues/02-proxy-forwarding-orchestration.md)：12 条清单（1 P2=终态失败零日志 / 11 P3），三项拍板：额度空候选 503 落库记失败行（含 NoMembers 补 warn）、直连成功清零保持现状只补注释、决策日志明细降 debug 留 info 选路结果。产物=findings/02-proxy-forwarding-orchestration.md。
- [03 · proxy 流式转运与指标记账审查](issues/03-proxy-stream-accounting.md)：9 条清单（1 P2=带内错误事件客户端假成功 / 8 P3），两项拍板：泵对转换器 error 态发 error 帧 + [DONE]（03-01 修复含 03-07 三协议回归；collect 非流式路径已正确可对照）、S3 request 表保留保持现状不清理（原 P1 读侧已被统计快照消除，登记体积观察项，复原形态存 revert 22bb5c8）。产物=findings/03-proxy-stream-accounting.md。

## Not yet specified

- 「P1 高危险发现」是否中途脱离清单模式插入修复（默认不改，图后统一排期）——由用户在票答覆盖时自行决定。
- 图后实施排期的组织方式（统一 backlog 文档？按严重度分批？按模块批量？）——终点之后的交付形态，图内不定。
- AGENTS.md 结构树刷新（漂移清单见 MODULES.md §1.4）与各票低危「口味级」条目的收敛决策——均归图后实施批次，图内不再开票。

## Out of scope

- 产品向工作：代际立项（adaptive thinking 等模型能力）、额度闸门恢复讨论、新厂商接入。
- 前端视觉/UX 评审（本图只审代码质量，不含设计走查）。
- 2026-09-08 及此前审计已整改项的复查（除非重构使其回归）。
