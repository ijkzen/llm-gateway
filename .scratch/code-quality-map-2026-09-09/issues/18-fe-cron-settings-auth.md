# 18 · FE 任务/设置域审查

Type: task
Status: open
Blocked by: 01

## Question

对前端任务/设置域做全量审查：`pages/cron-jobs.tsx` / `settings.tsx` / `not-found.tsx` + `components/cron-jobs/`（CronJobLogsDialog 等）+ `components/settings/` 组件族（SettingEditDialog/JsonSettingEditDialog/SettingsTable/ChangePasswordDialog/BackupDialog 等）+ use-cron-jobs/use-cron-job-logs/use-settings 及 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：SSE 日志流的快照/增量/重置状态机（MockEventSource 已锁行为不复查）、设置类型表单与后端声明类型一致性、弹窗滚动区约定（已收口不复查）；
- 实现简洁：表单构造重复、SSE hook 状态机复杂度；
- 测试覆盖：settings 域组件直测缺口（SettingEditDialog 无专属测试、settings.tsx 页无测试——01 盘点已标薄弱点）、CronJobLogsDialog 之外缺什么；
- 模块间调用：use-cron-job-logs 与后端 SSE 协议契约、settings 组件 i18n 绕过（49 处硬编码中文，归 20 票文案改造——本票只记录不重复）。

范围注记（01 盘点）：login/auth/chat 已拆至 21 票；hooks 为跨域共享接口，视为冻结。

产出 `.scratch/code-quality-map-2026-09-09/findings/18-fe-cron-settings-auth.md`，Answer 给摘要与需拍板问题。
