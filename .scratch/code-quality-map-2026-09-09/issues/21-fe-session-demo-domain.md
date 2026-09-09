# 21 · FE 会话与演示域审查

Type: task
Status: open
Blocked by: 01

## Question

对前端会话与演示域做全量审查：`pages/login.tsx` / `chat.tsx` + 认证守卫（RequireAuth/登录跳转） + use-auth / use-init-settings hooks 及 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：登录后跳转与守卫时序、初始化流程（首启 init/浏览器时区提交）、会话过期 401 全局跳转（lib/api.ts afterResponseHook，抽查已核）、chat 内联流式 fetch（chat.tsx:136，不经 ky/无共享 SSE hook）的错误分支与取消语义；
- 实现简洁：chat 流式状态机与 use-cron-job-logs 的 EventSource 差异面（同为流式消费，无共享抽象是否合理——01 盘点已确认两者零耦合）；
- 测试覆盖：login/RequireAuth 之外缺什么（chat 流式分支、401 钩子）；
- 模块间调用：use-auth 与后端 auth 契约、locale-toggle/use-locale 与 20 票 i18n 域的交界。

范围注记（01 盘点）：本票由原 18 票拆出；hooks 为跨域共享接口，视为冻结。
