# 15 · 系统工具与基础单体域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对系统工具与基础单体域做全量审查：`backup.rs` / `crypto/` / `config/` / `state.rs` / `i18n.rs` / `logs_cleanup.rs` / `response.rs` / `static_assets/` / `main.rs` / `lib.rs`（run 生命周期与 handler 注册）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：备份 JSON 解析/校验边界、i18n 占位符、优雅关闭时序、初始化失败路径、key 派生与加密格式版本；
- 实现简洁：各单文件内重复、跨文件的同构 helper（如时间/路径处理）；
- 测试覆盖：各模块单测之外缺什么（备份恢复演练、关闭竞态）；
- 模块间调用：handler 注册与 seed 双源一致性（08 票同查，此处查 lib.rs 侧）、state 聚合面是否合理。

产出 `.scratch/code-quality-map-2026-09-09/findings/15-system-utility-modules.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/15-system-utility-modules.md`——12 条（1 P2 + 11 P3）+ 一项拍板 + handler/seed 双源对账 4/4 一致。子代理深读 backup/config/lib 三大件，主代理直读六个小文件并复核全部保留发现行号。

**唯一 P2 = 15-01 优雅关停名存实亡**：`with_graceful_shutdown` 无超时，SSE 日志流（BroadcastStream 等 log_tx drop，而 AppState 持有到 run 返回=循环等待）与流式 /v1（分钟级）任一在飞，SIGTERM 后 serve 永不返回 → `scheduler.stop()`/worker 10s 收尾永不执行 → Docker SIGKILL 兜底硬杀 in-flight 任务（恰是 lib.rs 注释自称要避免的）。默认解=timeout 包 serve（如 8s 留收尾窗口）。

**拍板（用户当场拍定）**：15-02 APP_ENV 非法值静默回退 Dev（生产拼错 → 容器 cwd 新建空库「数据消失」）——**保持现状**（有测试固化为契约，事故面已知悉登记）。

**P3 要点**：15-03 备份成员唯一性不校验撞唯一索引报裸 SQL / 15-04 导出解密失败静默空串与 11-13 串联成「空凭据备份无声导出并成功导入」链（spec 认可语义，缺告警信号）/ 15-05 settings 导出无 order_by / 15-06 未知设置类型导出降级 String / 15-07 导入不清 request 表历史主体悬空（并 11-21 处置）/ 15-08 AGENTS.md example handler 文档漂移（实际 4 个）/ 15-09 i18n 插值两形态并存 / 15-10 static_assets 三微观察 / 15-11 db_error 直返 DbErr Display 观察 / 15-12 测试缺口六类（build_export 无单测、关停零回归等）。

**双源核对**：四个内置任务注册↔种子 4/4 对齐、注册先于加载无跳过窗口、seed 文案四分支齐；唯一不一致是 15-08 文档漂移。

**已核验无问题区**（9 项）：备份字段完整性+自然键、导入事务删除序、key_hash 重建口径、设置 upsert 不删键、启动序列与失败降级、关停序列本身正确（问题只在 serve 不返回）、logs_cleanup 安全形态、state/main 干净。

**需拍板问题**：无遗留（唯一取舍 15-02 已当场拍定）。

Status: resolved
