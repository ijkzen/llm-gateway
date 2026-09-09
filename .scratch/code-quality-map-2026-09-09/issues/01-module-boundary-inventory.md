# 01 · 模块边界全景盘点

Type: task
Status: resolved
Blocked by: —

## Question

以全仓为范围做一次模块边界盘点（后端 `src/` 与前端 `web/src/`，以 AGENTS.md 结构树为起点但不盲信），产出三样：

1. **模块图**：实际存在的功能模块、每个模块的公开接口面与主要消费方、跨模块调用关系（谁调谁、有无反向/循环依赖、有无隔层直调路由层等）。
2. **散落候选清单**：应成模块却散落各处的代码——重复实现的谓词/口径（已知教训：多份拷贝漂移族曾产出 3 个静默 bug）、散落在路由层的领域逻辑、跨模块共享却无处安放的 helper、无归属的原始 SQL 等，逐条给位置与证据。
3. **票切分修正建议**：02-20 的初版切分是否合理，需要合并/拆分/重排的给建议（含前端票粒度）。

结论落 `.scratch/code-quality-map-2026-09-09/MODULES.md`，Answer 给摘要。本票阻塞图上全部模块审查票——模块票以本票的切分为准，必要时增删改图内票。

## Answer

盘点完成（2026-09-09），产物 = `.scratch/code-quality-map-2026-09-09/MODULES.md`（模块图 + 生产依赖图 + 10 族散落候选 + 4 个文件归属结论 + 前后端票切分修正）。方法：模块级 import 图脚本全量统计（剥内联测试/文档注释）+ 前端子代理扫描 + 后端子代理扫描（重试一次成功）+ 载重锚点全部人工复核（约 30 处 sed/grep 对照磁盘，含一处子代理行号漂移修正：RecheckGate trigger 实际在 lb.rs:47 非 :56）。

关键结论摘要：

- **模块图**：后端 20 模块两清分层（底层 entity/response/i18n/crypto/config 零出边；routes 为唯一入口层直连能力层；state/lib 组合根健康）；availability 实际是纯 entity 依赖的 354 行状态机底座（生产 API 全部核实，消费横跨 proxy/routes/usage）；唯一双向边 usage↔proxy 集中在 persist.rs:258/279/299 边界探活一处（by-design）。
- **10 族散落候选**：F3（600s 新鲜度）与 F6（探活构造）已单源免票；F1 disabled_reason 字面量 4 处绕开已有枚举、F2 run 状态裸 String 零单源、F4 时区偏移表达式 3 份、F5 协议编号分类法三写 + ProtocolType 枚举死码、F7 usage:true 谓词双实现、F8 URL 版本段双规则 + 误导注释、F9 request 表原始 SQL 多套无 helper、F10 proxy/usage 中文文案不走 i18n——各有主票归属（MODULES.md §4）。
- **文件归属**：proxy/failure_recovery.rs 名不副实成立（唯一消费=cron handler 注册，主票 08 审归属）；failure_recheck.rs 位置合理；usage 子模块（estimate/error/http）边界干净。
- **切分修正已落票**：后端 08 纳入寄居文件、11 补 5 个漏网路由文件；前端 18 拆出 21（会话/演示域）、19 缩小（hooks 归消费域）、16/17 加 hooks 冻结接口注记、20 显式含 settings 文案域。
- **文档漂移**：AGENTS.md 结构树落后于磁盘（routes/usage/provider_model/stats_snapshot/cron 多处未收录）——随图后实施批次统一刷新。
