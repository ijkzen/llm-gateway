# 03 · proxy 流式转运与指标记账审查

Type: task
Status: resolved
Blocked by: 01

## Question

对 proxy 流式转运与记账域做全量审查：`relay.rs`（统一转运泵/Converter 门面）/ `dispatch.rs`（dispatch_success/accumulate_chunks/record_failure）/ `metrics.rs` / `sse.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：流中断/转换失败/超时的成功失败记账口径、ttft/tps 口径、落库竞态、Converter 门面与 TailSpec 收尾策略边界；
- 实现简洁：流泵三参数协议是否有残余分叉、死代码；
- 测试覆盖：畸形事件/中断回归之外还缺哪些分支锁定；
- 模块间调用：与 02/04/05 票域的边界是否合适。

**归位遗留项 S3**：request 表保留策略与 rollup（现状=代码已 revert 为无保留机制、表只增不减）——在此给出重估结论（是否建议入清单、推荐形态如保留期清理/rollup/不动）。

产出 `.scratch/code-quality-map-2026-09-09/findings/03-proxy-stream-accounting.md`，Answer 给摘要与需拍板问题。

## Answer

已解决（2026-09-10）。产物=findings/03-proxy-stream-accounting.md（9 条清单 + S3 重估结论 + 已核验区 + 性能轮结论，行号锚点全部复核）。

- **清单**：1 P2（03-01 带内错误事件客户端假成功——转换器 error 态只落库不给客户端 error 帧，[DONE]-only 收尾）+ 8 P3（03-02 post-[DONE] teardown 翻转记账、03-03 dispatch/relay 零日志同族续点并入 02-01 批、03-04 事件循环双份分叉、03-05 AG 非流式读体失败原因被吞、03-06/03-07 测试缺口、03-08 splitter 无上限、03-09 usage 过滤形态过窄）。
- **拍板两项**：
  1. 03-01 修复——泵对转换器 error 态发 OpenAI error 帧 + [DONE]（与畸形事件/断流同形），补三协议带内错误回归（03-07）；collect 非流式路径已正确可对照。
  2. S3 request 表保留——保持现状不清理：原 P1 读侧已被统计快照（ADR-0021）消除，全史明细保留对账价值，rollup 与快照重复不成立；实施批登记体积观察项（>1 千万行或 >2GB 再评估），复原形态存于 revert 22bb5c8。
- **已核验**：客户端断开 success=1+原因补记与 entity 文档一致（设计）；四臂记账全部经 P4 单写者；ttft/tps 口径与 entity 注释逐条一致；泵骨架收拢成效确认（PumpSource/TailSpec 枚举封闭、无死代码）；reasoning 剥除双侧一致。
