# 21 · FE 会话与演示域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对前端会话与演示域做全量审查：`pages/login.tsx` / `chat.tsx` + 认证守卫（RequireAuth/登录跳转） + use-auth / use-init-settings hooks 及 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：登录后跳转与守卫时序、初始化流程（首启 init/浏览器时区提交）、会话过期 401 全局跳转（lib/api.ts afterResponseHook，抽查已核）、chat 内联流式 fetch（chat.tsx:136，不经 ky/无共享 SSE hook）的错误分支与取消语义；
- 实现简洁：chat 流式状态机与 use-cron-job-logs 的 EventSource 差异面（同为流式消费，无共享抽象是否合理——01 盘点已确认两者零耦合）；
- 测试覆盖：login/RequireAuth 之外缺什么（chat 流式分支、401 钩子）；
- 模块间调用：use-auth 与后端 auth 契约、locale-toggle/use-locale 与 20 票 i18n 域的交界。

范围注记（01 盘点）：本票由原 18 票拆出；hooks 为跨域共享接口，视为冻结。

## Answer

**清单产出**：`findings/21-fe-session-demo-domain.md`——4 条（1 P2 + 3 P3）无拍板。域小（891 行）主代理直读无子代理。

**唯一 P2 = 21-01 初始化保存竞态**：`saveInitSettings` 在 init mutation 启动即同步发两个 PUT，而 init 端点 argon2 哈希（数百 ms）后才建会话——PUT 必然先到被拒 401，首启选的时区静默丢失落回种子默认，成功路径还弹「保存失败」toast。注释只意识到 init 失败的半面。默认解=移到 onSuccess（一行挪动）。

**P3**：21-02 chat 静默吞 03-01 流内 error 帧（无 choices 被 deltaOf 忽略，内容戛然而止无错误标记）/21-03 eventData 单行 data 假设（当前不可达观察）/21-04 测试缺口族（21-01 时序无回归网、use-auth 三 mutation 零直测、chat error 帧/畸形 chunk 未测）。补充登记：init 表单 zod 长度 UTF-16 vs 后端字节=18-15 同族。

**已核验无问题区**：RequireAuth 守卫（有测试）与全局 401 双通道、useAuthAction setQueryData 双写、useLogout 失败收敛、chat 主链路（类型/输入法/abort/reasoning_details 回传）、chat 不经 ky 的 401 形态可接受、时区选项生成正确。

**需拍板问题**：无。

Status: resolved
