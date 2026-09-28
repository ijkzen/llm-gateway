# 03: 每日定时任务接线

**What to build:** 内置定时任务 `models_refresh`（表达式 `0 0 4 * * *`，本地时区，即生产东八区每天 04:00）出现在定时任务页并每天自动执行：启动期把外置目录路径注入读取缓存，handler 调用刷新函数从 models.dev 拉取并原子写入外置目录；失败保留旧文件并把本次执行记为失败（附错误日志）；管理员可在页面点「立即执行」手动触发。无外置路径时告警并正常返回，不主动联网预热。

**Blocked by:** 01、02。

**Status:** ready-for-agent

- [ ] `cron::seed` 增加 `models_refresh` 常量、中英文默认标题与描述、幂等种子（`0 0 4 * * *`、`group=system`、`enabled=true`）。
- [ ] `lib.rs` 启动时由 `DATABASE_URL` 推导外置路径并注入 catalog 全局实例；注册 `models_refresh` handler（进程级 `try_lock` 防重叠）。
- [ ] handler 成功记 info（含条目数）；失败返回错误交回 worker（run 记 failed + 追加「任务执行失败：…」系统日志），旧外置文件不被破坏。
- [ ] 刷新成功后无需重启，后续匹配即用新目录（依赖 01 的 mtime 缓存）。
- [ ] 无外置路径（如内存库）时告警并正常返回。
- [ ] 集成测试：种子幂等、表达式、group、enabled、`load_from_db` 后 `has_job` 为真（参照 usage_refresh 种子测试）。

**Acceptance criteria 验证方式:** `cargo test --all-targets` 全绿；定时任务页可见 `models_refresh` 并可手动执行。
