# 13 · 数据与实体层审查

Type: task
Status: claimed
Blocked by: 01

## Question

对数据层做全量审查：`db.rs`（1202 行迁移单体：连接配置/WAL/建表/26 版增量迁移/ANALYZE/ensure_sqlite_dir）、`entity/` 全目录及迁移相关测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：迁移幂等/版本守卫/废弃号段坑（16 起编惯例）、WAL 与 busy_timeout 配置、schema 与 entity 漂移面；
- 实现简洁：迁移单体拆分必要性与风险（对照单文件 ≤1000 行约定——db.rs 已超限，评估拆分方案）、连接参数重复；
- 测试覆盖：26 快照两表测试之外缺什么（坏库恢复、并发迁移）；
- 模块间调用：entity 被全仓直用是否形成合理边界、是否有跨实体事务散落在调用方。

产出 `.scratch/code-quality-map-2026-09-09/findings/13-data-entity-layer.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/13-data-entity-layer.md`——6 条全 P3，无 P1/P2、无拍板项。主代理直接全读（db.rs 1202 行 + entity 16 文件），无子代理。

**六条 P3**：13-01 并发首启迁移撞车（版本检查在事务内但快照隔离下双读 count=0，后行者报错退出；fail-fast 不坏库、部署串行替换不可达；注释「prevents concurrent execution」过度声称）/ 13-02 迁移 1 else 分支返回值未并入 changed（补记版本号时 ANALYZE 被跳过，仅影响计划统计）/ 13-03 双轨 schema 收敛只靠纪律（provider_schema_check 只覆盖 provider 一表；已有漂移实例=snapshot_meta.updated_at 实体建 timestamp_with_timezone_text vs 迁移 DDL text，affinity 相同无行为差异）/ **13-04 db.rs 超行评估=采纳测试迁出**（1202 行中 558 是 cfg(test)，非测试 644 已达标；默认解=测试迁 src/db/tests.rs 子模块，cron/scheduler/tests.rs 有先例、纯搬运零风险；迁移主链 454 行不再拆）/ 13-05 新库死列加删抖动（迁移 17 加 22 删 failure_disabled，线性链代价无害）/ 13-06 测试缺口四类（并发迁移需文件库/坏库半迁移窗口/迁移 1-12 老库模拟缺/ensure_migration 回滚路径）。

**已核验无问题区**（8 项）：版本守卫同事务+DDL 事务回滚+九版幂等测试、废弃号段 14/15 与单次 ensure_migration 两教训均注释化且有撞号回归、连接配置单处集中（含 256MB→62.5MiB 注释修正自觉）、ANALYZE 仅变更后、sqlite_url_path 五形态有回归、column_exists 无注入面、实体零行为纯 schema（request 口径注释详尽含短回复缓冲坑）、entity 直用是一致风格非泄漏。

**性能轮**：无负债（启动迁移一次性毫秒级；连接池/pragma 组合理；热路径不经 db.rs）。

**需拍板问题**：无（13-04 拆分方案已按先例给出明确默认解，机械搬运无取舍）。

Status: resolved
