# FINDINGS · 15 系统工具与基础单体域审查（2026-09-10）

范围：`backup.rs`（837 全读）、`config/mod.rs`（312 全读）、`lib.rs`（320 全读）、`state.rs`（34）、`i18n.rs`（49）、`logs_cleanup.rs`（105 含 4 测试）、`response.rs`（85）、`static_assets/mod.rs`（81）、`main.rs`（10）+ 交叉核对（seed.rs 种子清单、cron SSE 端点、tests/backup_integration）。crypto 已在 12 票审过（12-03/12-04 不重复）。方法：一个子代理深读 backup/config/lib 三大件 + 主代理直读六个小文件 + 所有保留发现行号磁盘复核。清单模式：不改代码。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 15-01 | P2 | 逻辑/关停 | 优雅关停无超时：SSE 日志流与流式 /v1 长连接钉死 `serve`，`scheduler.stop()`/worker 10s 收尾永不执行（Docker SIGKILL 兜底），与 AGENTS.md 关停承诺不符 |
| 15-02 | P3 | 配置/观察 | APP_ENV 非法值静默回退 Dev → 生产容器内相对路径新建空库「数据消失」——**已拍板保持现状**（有测试固化；登记观察，用户知悉事故面） |
| 15-03 | P3 | 校验 | `validate_backup` 不校验成员唯一性（同模型重复引用），导入撞 `uq_virtual_model_items_model_id` 返回裸 SQL 错误 |
| 15-04 | P3 | 健壮 | 备份导出解密失败一律 `unwrap_or_default()` 空串（spec 认可的设计），但与 11-13（导入不校验空 apiKey）串联成「密钥全空的备份能无声导出并成功导入」链 |
| 15-05 | P3 | 简洁/确定性 | 备份导出六表唯独 settings 无 `order_by`（backup.rs:145），备份文件字段序不稳定 |
| 15-06 | P3 | 逻辑/保真 | 未知 `setting.type` 导出为 `"String"`，再导入被静默改写为 type=0 |
| 15-07 | P3 | 模块间 | 备份导入整表替换不清 `request` 表：历史指标行 provider_id/virtual_model_id 全部悬空（11-21 孤儿面的新一侧，建议并入同一清理定夺） |
| 15-08 | P3 | 文档 | AGENTS.md 称注册了 `example` 示例 handler，实际 lib.rs 只注册 4 个（usage_refresh/failure_recovery/stats_snapshot×2）——文档漂移，归图后文档批次 |
| 15-09 | P3 | 简洁/i18n | 插值消息两形态并存：i18n 注释约定 `format!(lang.tr(...含{}...))`，调用点实为 if/else 各语言 format!（cron_jobs 等）；`en-US`→En 解析与 Display→"en" 不对称（微） |
| 15-10 | P3 | 观察/微 | static_assets 三微：`is_hashed_asset` 按目录而非哈希存在性、etag_matches 不认 `*`/weak 前缀、304 响应不带 ETag/Cache-Control 回头 |
| 15-11 | P3 | 安全观察/微 | `response::db_error(e.to_string())` 把 DbErr Display（部分变体含 SQL 片段）直返客户端——管理端单用户面，记观察 |
| 15-12 | P3 | 测试覆盖 | 缺口六类：build_export 无单测/导出解密失败路径零覆盖/导入回滚注入点无单测/并发导入/大备份 413/导入后运行时一致性（时区 reload、缓存热更）；config 边界与 lib.rs 关停零回归 |

## 各条证据

### 15-01 优雅关停无超时，长连接钉死收尾（P2，关停时序）

lib.rs:281-290：`axum::serve(...).with_graceful_shutdown(shutdown_signal()).await?` ——该语义是「停止 accept + 等全部既有连接自然结束」，**无超时**；`scheduler.stop()` 与 `worker.shutdown(10s)` 排在 serve 返回之后。两条长连接面：①SSE 日志流（cron_jobs.rs:376-378，BroadcastStream 只在 log_tx 全 drop 后结束，而 AppState 持有 log_tx 到 run 返回——循环等待）；②流式 /v1 转发（单条流可达分钟级）。任一在飞即 SIGTERM 后 serve 不返回 → 10s 收尾永远不执行 → Docker stop 超时 SIGKILL 硬杀，in-flight 任务中断——恰是 lib.rs:285-286 注释自称要避免的。缓解面=SIGKILL 兜底不死进程，但「优雅关闭」名存实亡。默认解：`tokio::time::timeout` 包 serve（如 8s，留出 worker 收尾窗口），超时后不再等连接直接进收尾；或给 SSE/流式接 shutdown 通知（改动大，非首选）。

### 15-02 APP_ENV 非法值静默回退 Dev（P3，已拍板保持现状）

config/mod.rs:18-25 FromStr 只认 dev/prod（lowercase 后，`production` 不匹配）；:40-43 `.and_then(parse().ok()).unwrap_or(Dev)`；Dev 默认库=相对路径 `sqlite://db/app.db`。生产拼错 → 容器 cwd 新建空库照常启动、日志写相对 logs/——面板「数据消失」而真库无恙。`test_config_invalid_app_env_defaults_to_dev`（:280-294）固化为契约。**拍板（2026-09-10）：保持现状**，登记观察，事故面已知悉。

### 15-03 备份成员唯一性未校验（P3，校验）

validate_backup 的成员循环（backup.rs:371-399）只查引用存在+协议匹配，不查同次导入内 `(providerName, providerModelId)` 重复；apply_import 逐条 insert 撞 `uq_virtual_model_items_model_id`（db.rs:248）→ `DbErr` 原文经 map_err 进 400 消息。事务回滚不脏库，仅错误消息不友好。默认解：validate 阶段 HashSet 检出重复。

### 15-04 导出解密失败静默空串（P3，健壮）

backup.rs:174/:178/:219 三处 `crypto::decrypt(...).unwrap_or_default()`。密钥轮换/迁移机器后导出结构完整、校验全过、凭据全空；导入侧 11-13 不拦空 apiKey → 空密钥写回库。spec 明文认可「不可解的密文导出为空」，非意外；缺的是信号。默认解：build_export 收集解密失败计数，导入/导出响应带回 warning 字段。

### 15-05 settings 导出无排序（P3，确定性）

backup.rs:145 `setting::Entity::find().all(db)` 无 order_by；其余五表都有（:125-144）。备份文件 settings 序不稳定，导出→再导出不可逐字节比对。默认解：`order_by_asc(setting::Column::Key)`。

### 15-06 未知设置类型导出降级 String（P3，保真）

backup.rs:246-250 `setting_type_name` 对 try_from 失败的未知 i32 映射 String → 再导入按 type=0 落库，静默改写。仅历史脏数据可达，观察级。

### 15-07 导入后 request 历史悬空（P3，模块间）

apply_import（backup.rs:430-450）清五张配置表不碰 request；导入后 request.provider_id/virtual_model_id 指向已删行（AUTOINCREMENT 不复用），赛马/面板按主体归并缺主体名。spec 把请求历史列为不导出（合理），但未声明「保留悬空引用」是预期。建议并入 11-21 的「导入后派生/历史表处置」一并定夺，不单独开修。

### 15-08 example handler 文档漂移（P3，文档）

AGENTS.md「Handler 注册」称注册了 `example` 示例；lib.rs 实际只有 4 处 register_handler（:164/:196/:220/:232），全仓 grep 无 example，seed 侧亦无。归图后 AGENTS.md 刷新批次（MODULES.md §1.4 漂移清单）。

### 15-09 i18n 插值两形态（P3，简洁）

i18n.rs:41-42 注释约定「占位符 {} 保留，调用方 format!」；实际插值消息多为 if/else 各语言 format!（如 cron_jobs.rs:183-190）。两种形态并存，无功能问题，风格债。另 `en-US` 解析为 En 而 Display 输出 `en`，写回设置值时不对称（settings 校验接受 en-US 但存的是原值——微观察）。默认解：图后统一为 tr+format!（可选）。

### 15-10 static_assets 三微观察（P3）

static_assets/mod.rs：①`is_hashed_asset`（:11-13）按 `assets/` 目录判定而非文件名含哈希（vite 默认全部产物带哈希，现状安全，约定弱）；②`etag_matches`（:46-51）只认精确值，不认 `*` 与 `W/` 弱校验（浏览器实际发精确值，无影响）；③304 响应（:38-43）不回带 ETag/Cache-Control（RFC 建议带，浏览器容错）。默认解：保持现状，登记。

### 15-11 db_error 直返 DbErr Display（P3，安全观察/微）

response.rs:68-70 `db_error(e.to_string())`——SeaORM DbErr 部分变体 Display 含 SQL 语句片段，直返客户端。管理端单用户、会话保护，风险低；若未来多用户化需收敛。默认解：保持现状（单用户前提下），登记。

### 15-12 测试缺口（P3）

已有：backup.rs 内 18 单测（parse/validate 矩阵）+ backup_integration 9 用例 + logs_cleanup 4 用例 + config 用例（含 APP_ENV 回退锁定）+ response/i18n 极薄无专测（结构简单可接受）。缺口：T1 build_export 无单测+解密失败路径零覆盖（15-04）；T2 apply_import 注入失败点的回滚单测；T3 并发导入（SQLite 写锁竞争表现）；T4 大备份 >5MB 413；T5 导入后运行时一致性（时区 reload_all_jobs、settings 缓存热更、cron/会话不受影响）；T6 关停顺序与 SSE 挂起（15-01）零回归；config 的 BIND_ADDRESS 格式/CRON_JOB_* 空白串边界。

## handler 注册与 seed 双源核对（票内问题 4 的正面对账）

四个内置任务 4/4 对齐：usage_refresh（lib.rs:163-191 ↔ seed @every 5m）、failure_recovery（:195-215 ↔ @hourly）、stats_snapshot（:219-230 ↔ @every 1h）、stats_snapshot_rebuild（:231-242 ↔ @every 1h）。「注册了没种子」「种子了没注册」两侧皆空；注册先于 load_from_db（:261），无「有种子无 handler 被跳过」窗口；seed.rs 默认文案四分支一一覆盖。唯一不一致=15-08 文档漂移。

## 已核验无问题区（避免后续票重复审查）

- **备份导出字段完整性**：六表逐字段比对实体无遗漏；成员用 (providerName, providerModelId) 自然键跨库稳定；生效协议口径（protocol_type ?? provider.protocol_type）与 DB 侧 effective_protocols 逐字同构，Full Compatible 豁免一致。
- **导入事务与删除序**：成员→虚拟模型→模型→供应商→API Key 依赖序正确，任一点失败 txn drop 回滚。
- **API Key 重建**：key_hash=hash_token(明文) 与 auth 口径一致（roundtrip 测试锁定可被 Bearer 命中）。
- **设置 upsert 语义**：只覆盖出现的键、不删除（spec 一致，测试在）。
- **启动序列**：迁移→key_hash 回填→extra 加密回填→模板 upsert→AppSettings 加载→残留 running 标 failed→worker/scheduler→handler 注册→种子→快照预热 spawn→加载任务→start→日志清理；各 backfill 失败降级 warn 不中断启动。
- **优雅关停序列本身正确**（serve→scheduler.stop→worker.shutdown(10s)），问题只在 serve 可能不返回（15-01）。
- **logs_cleanup**：未来 mtime 文件安全跳过、非递归（目录卫生文档化）、启动即跑一次（interval 首拍立即触发）、4 测试含 FileTimes 老化。
- **state.rs**：聚合面干净、各字段注释到位。
- **main.rs**：极简，config 失败 expect 退出码 1。

## 性能/内存轮结论

无 P1/P2。备份导出六表全量内存组装（O(P×M)/O(V×I) 内存 filter，配置表十~百量级无碍）；导入逐行 insert 同事务；config 纯 env 解析；启动序列主体串行、快照预热异步化（就绪优先）；setup_logging 双 JSON layer+非阻塞 appender 常量开销。结论：本域无性能负债。
