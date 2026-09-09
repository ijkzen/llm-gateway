# 03 · proxy 流式转运与指标记账审查

Type: task
Status: open
Blocked by: 01

## Question

对 proxy 流式转运与记账域做全量审查：`relay.rs`（统一转运泵/Converter 门面）/ `dispatch.rs`（dispatch_success/accumulate_chunks/record_failure）/ `metrics.rs` / `sse.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：流中断/转换失败/超时的成功失败记账口径、ttft/tps 口径、落库竞态、Converter 门面与 TailSpec 收尾策略边界；
- 实现简洁：流泵三参数协议是否有残余分叉、死代码；
- 测试覆盖：畸形事件/中断回归之外还缺哪些分支锁定；
- 模块间调用：与 02/04/05 票域的边界是否合适。

**归位遗留项 S3**：request 表保留策略与 rollup（现状=代码已 revert 为无保留机制、表只增不减）——在此给出重估结论（是否建议入清单、推荐形态如保留期清理/rollup/不动）。

产出 `.scratch/code-quality-map-2026-09-09/findings/03-proxy-stream-accounting.md`，Answer 给摘要与需拍板问题。
