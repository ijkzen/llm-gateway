# FINDINGS · 13 数据与实体层审查（2026-09-10）

范围：`db.rs`（1202 行全读：连接配置/16 表建表/迁移 1-26/三助手/558 行测试）、`entity/` 全目录 16 文件（重点 provider/request/snapshot/snapshot_meta/setting/cron_job 逐行，其余扫读）+ 交叉核对（tests/provider_schema_check.rs、dev 库实际 schema、entity↔迁移 DDL 收敛性）。方法：主代理直接全读，无子代理。清单模式：不改代码。**本票无 P1/P2、无需拍板项**（全部默认解明确）。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 13-01 | P3【已修复（注释）2026-09-10】 | 逻辑/健壮 | 并发首启迁移撞车：两进程同库同时 migrate，版本检查在事务内但 SQLite 快照隔离下双读 count=0，后行者 PK 冲突或重复列报错退出——fail-fast 不坏库，部署串行替换下不可达；代码注释（582-584）声称「prevents concurrent execution」过度声称 |
| 13-02 | P3【已修复 2026-09-10】 | 逻辑/微 | 迁移 1 的 else 分支（列已存在只补版本记录，:209）返回值未并入 `changed`——补记版本号时 ANALYZE 被跳过（仅影响查询计划统计，非正确性） |
| 13-03 | P3【已修复 2026-09-10】 | 测试覆盖 | schema 双轨（新库=实体建表、老库=迁移 ALTER）收敛只靠纪律：provider_schema_check 只覆盖 provider 一表；已有漂移实例=snapshot_meta.updated_at 实体建为 `timestamp_with_timezone_text`、迁移 26 DDL 写 `text`（affinity 相同无行为差异） |
| 13-04 | P3【已实施拆分 2026-09-10】 | 简洁 | db.rs 1202 行超 ≤1000 行约定，其中 558 行是 #[cfg(test)]（非测试 644 行）；默认解=测试迁 `src/db/tests.rs`（`cron/scheduler/tests.rs` 有先例），纯搬运零行为变化 |
| 13-05 | P3【观察级保持现状 2026-09-10】 | 简洁/微观察 | 新库首启时迁移 17 先加 `failure_disabled`、迁移 22 再删——线性迁移链的死列加删抖动（一次性、幂等、无害） |
| 13-06 | P3【已部分修复 2026-09-10】 | 测试覆盖 | 缺口四类：并发迁移撞车（内存库测不到需文件库）/坏库与半迁移恢复（ALTER 成功版本插入失败的崩溃窗口）/迁移 1-12 老库模拟全缺（9 版有直测的是 13/16/18/19/21/22/23/24/26）/ensure_migration 语句失败回滚不记版本无直测 |

## 各条证据

### 13-01 并发首启迁移撞车（P3，健壮）【已修复（注释）2026-09-10】

`ensure_migration`（db.rs:607-642）版本检查 `SELECT COUNT(*) WHERE version=?` 与执行都在同一事务内，但 SQLite DEFERRED 事务快照隔离下两个并发连接可同时读到 count=0 → 都执行 ALTER/CREATE → 后行者撞「重复列」或 schema_migrations 主键冲突而报错，`connect()` 失败进程退出。失败显式、不损坏库（DDL 在事务内回滚），且现实部署（deploy.sh 串行替换容器）不可达，P3。注释（:582-584）称「the in-transaction migration guard prevents concurrent execution」过度声称——守卫不防止并发，只是把并发变成 fail-fast。默认解：注释改实；如需真正并发安全，版本检查改为 `INSERT OR IGNORE ... RETURNING` 抢占式占位（图后再议）。

### 13-02 迁移 1 else 分支丢弃 changed（P3，微）【已修复 2026-09-10】

db.rs:204-210：迁移 1 列已存在时 `ensure_migration(db, 1, &["SELECT 1"]).await?` 返回值未 `changed |=`（其余所有迁移均并入）。该分支首次补记版本号时 changed 不报真 → `connect()` 跳过 ANALYZE（:99-102）。ANALYZE 只影响查询计划统计，且后续任何迁移变更都会补跑，P3 微。默认解：补 `changed |=`。

### 13-03 双轨 schema 收敛只靠纪律 + 已有一例类型名漂移（P3，测试覆盖）【已修复 2026-09-10】

新库 schema 来自 `create_table_from_entity`（16 表），老库来自迁移 ALTER/CREATE；两者收敛没有任何系统性守卫，唯一对齐测试 `tests/provider_schema_check.rs` 只覆盖 provider 一张表（列名/类型/默认值三元组比对）。漂移实例已存在：snapshot_meta.updated_at 新库由实体建为 `timestamp_with_timezone_text`（sea-query 对 DateTimeUtc 的 SQLite 映射，dev 库 provider.created_at 同款可证），迁移 26 DDL 写 `text`（db.rs:550）——两者 TEXT affinity 相同、零行为差异，但说明「实体与迁移 DDL 双写」必然漂移。默认解：图后把 provider_schema_check 泛化为全表（或至少快照两表+request）逐列比对，成本一行宏循环。

### 13-04 db.rs 超 1000 行约定的拆分评估（P3，简洁）【已实施拆分 2026-09-10】

现状 1202 行 = 连接与配置 105 + 迁移主链 454 + 三助手 85 + 测试 558。非测试部分 644 行已达标。拆分方案对比：①测试迁 `src/db/tests.rs`（db.rs 转为 `src/db/mod.rs` 门面）——纯搬运、测试全部用私有 fn（`migrate`/`column_exists`/`sqlite_url_path` 均 pub(crate)/私有，子模块 `use super::*` 原样可用），`cron/scheduler/tests.rs` 有同款先例，风险≈0，迁后 644 行达标；②迁移主链再拆 `db/migrations.rs`——收益低（主链 454 行已远低于阈值）且割裂迁移叙事的线性可读性，不建议。结论（记录待实施批）：采纳①，不采纳②。

### 13-05 新库死列加删抖动（P3，微观察）【观察级保持现状 2026-09-10】

新库实体建表已无 `failure_disabled`，但迁移 17 `ensure_columns` 检测到缺列仍会 ADD，迁移 22 再 DROP——线性迁移链对「已被实体吸收又删除」的列产生一次性加删。幂等、空表零回填、无危害；仅启动日志多两条 ALTER。默认解：保持现状（改链=重写历史迁移，违反迁移不可变惯例）。

### 13-06 迁移测试缺口（P3，测试覆盖）【已部分修复 2026-09-10】

已有直测：13（两次调用撞号回归）/16（废弃号段 14/15 撞号）/18×2/19×2/21（回填三态+幂等）/22/23/24×2/26×2 + url_path 三例 + ensure_dir 一例 + 快照表 UNIQUE 拒绝重复行。缺口：
- T1 并发迁移撞车（13-01）：需文件库 + 双连接并发 migrate，内存库无 WAL 语义测不到（517 教训同款测试缝）。
- T2 坏库/半迁移恢复：ALTER 成功但版本插入失败的崩溃窗口（进程被杀于事务中——SQLite 事务回滚保证原子，实际窗口=无，但无测试锁定该保证）；schema_migrations 行损坏（version 冲突/负值）行为未测。
- T3 迁移 1-12 老库模拟全缺：group/is_deleted（1）、key_hash（7）、sort_order（11）、索引升级（3/12）等老库路径无直测。生产已越过这些版本，回补价值递减，记观察。
- T4 ensure_migration 语句失败路径（如 ALTER 语法错）回滚且不记版本、下次重试——无直测。

## P3 实施批（2026-09-10）

- **13-01 已修复（注释）**：迁移守卫注释由「in-transaction migration guard prevents concurrent execution」改为如实描述——版本守卫把并发变成 fail-fast（DDL 随事务回滚、库不会半迁移），并不序列化并发首启；部署侧是串行单容器。真正并发安全（`INSERT OR IGNORE ... RETURNING` 抢占）保持图后另议。
- **13-02 已修复**：迁移 1 的 else 分支补 `changed |=`——该路径不再跳过启动 ANALYZE。
- **13-03 已修复**：新增全表 schema 对齐守卫 `snapshot_and_request_tables_have_matching_columns_between_paths`——对 `request` / `request_log_snapshot` / `snapshot_meta` 三表比对「实体建表」与「迁移链（走 `db::connect` 真实启动路径）」的列集合（必须完全一致）与 affinity（容忍 `timestamp_with_timezone_text` vs `text` 这类类型名漂移）。实测三表零漂移，只有已知的类型名差异。
- **13-04 已实施拆分**：`db.rs` 的 558 行测试迁至 `src/db/tests.rs`（`#[path]` 挂回，`cron/scheduler/tests.rs` 同款先例），`db.rs` 由 1202 行降至 651 行（主链 + 声明），符合单文件 ≤1000 行约定；零行为变化，18 例原测试全绿。
- **13-05 观察级保持现状**：新库对「已被实体吸收又删除」的列（failure_disabled）仍会一次性 ADD→DROP，幂等无危害；改链违反迁移不可变惯例。
- **13-06 已部分修复**：新增 2 条迁移测试——失败迁移整体回滚且不记版本（`ensure_migration_rolls_back_and_skips_version_on_failure`）、版本已记录时 no-op 且 changed=false（`ensure_migration_is_noop_when_version_recorded`）。并发撞车（需文件库 + 双连接并发）与 1-12 老库模拟保留后续。


## 已核验无问题区（避免后续票重复审查）

- **版本守卫与幂等**：ensure_migration 检查-执行-记版同事务；DDL 在 SQLite 事务内可回滚；「缺列检查在事务外、执行在事务内」的两段式对单进程启动安全。重复执行幂等有九版测试锁定。
- **废弃号段惯例已注释化**：迁移 16 头注释明示 14/15 被旧 lg-proxy 占用、新迁移从 16 起编（db.rs:369-370）；迁移 13 的「缺失列必须合并单次 ensure_migration」教训同样注释化（:344-346）。两坑都有撞号回归测试（16/18/19 三例带 14/15 残留记录）。
- **连接配置单处集中**：max_connections 5 / busy_timeout 5s（注释明示「过长掩盖锁竞争」，与 517 修复史一致）/ WAL / synchronous NORMAL / 外键 / cache_size -64000≈62.5MiB/连接（注释含 256MB 误算修正自觉）/ temp_store=内存 / journal_size_limit 64MB / wal_autocheckpoint 1000 / mmap 256MB / 慢查询 100ms warn。无第二处连接参数。
- **ANALYZE 仅变更后**（:99-102），新库首启必跑（changed=true）。
- **sqlite_url_path**：sqlite://相对/sqlite:///绝对/sqlite:裸路径/:memory:/带 query 五形态正确，绝对路径防回退有回归测试（:675-683）。
- **column_exists/is_unique_violation**：PRAGMA 表名全内部常量无注入面；is_unique_violation 串匹配对 SQLite-only 项目可接受（全仓消费方均在错误分支）。
- **实体层零行为纯 schema**：request 实体 19 字段口径注释详尽（ttft/tps/output_tokens_time 含短回复缓冲压缩的已知坑）；provider 实体 disabled_reason 注明 ADR-0003 镜像不变式与 availability 单点写入；setting 的 SettingType 枚举三向转换（i32/Display/FromStr）齐备。provider 有唯一约束 name、virtual_model 有唯一 display_id、api_key 唯一 name、cron_job_run 唯一（job_name, ...）等关键约束在实体声明。
- **entity 直用边界**：全仓 routes/proxy/usage/cron 直用实体是本项目一致风格（仅 cron 有 Repository trait 封装），实体无行为故无泄漏面；跨实体事务散落调用方已在 11 票（11-04/11-06/11-21）记录，不重复。

## 性能/内存轮结论

无性能项。启动迁移=16 次 CREATE IF NOT EXISTS + 逐版本检查，串行一次性毫秒级；column_exists 每列一次 PRAGMA，启动路径可忽略；连接池参数与 SQLite pragma 组合理（上条）；运行期热路径不经过 db.rs（连接池复用）。内存面：cache_size 62.5MiB/连接 ×5 上限约 0.3GB + mmap 256MB 地址空间（非驻留），注释已自觉修正早先误算。结论：本域无性能负债。
