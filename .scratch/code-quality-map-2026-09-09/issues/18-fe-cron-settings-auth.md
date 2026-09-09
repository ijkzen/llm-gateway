# 18 · FE 任务/设置/认证域审查

Type: task
Status: open
Blocked by: 01

## Question

对前端任务/设置/认证域做全量审查：`pages/cron-jobs.tsx` / `settings.tsx` / `login.tsx` / `chat.tsx` / `not-found.tsx` + `components/cron-jobs/`（CronJobLogsDialog 等）+ 认证守卫（RequireAuth/login 流程 hooks）及 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：SSE 日志流的快照/增量/重置状态机（MockEventSource 已锁行为不复查）、登录后跳转与守卫时序、设置类型表单与后端声明类型一致性、弹窗滚动区约定（已收口不复查）；
- 实现简洁：表单构造重复、SSE hook 状态机复杂度；
- 测试覆盖：login/RequireAuth/ChangePasswordDialog 之外缺什么；
- 模块间调用：use-cron-job-logs 与后端 SSE 协议契约、i18n 占位符使用面。

产出 `.scratch/code-quality-map-2026-09-09/findings/18-fe-cron-settings-auth.md`，Answer 给摘要与需拍板问题。
