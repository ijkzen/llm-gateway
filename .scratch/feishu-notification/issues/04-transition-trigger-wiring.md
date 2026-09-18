# 04: 通知触发接线 —— 自动转换发消息

**What to build:** 供应商发生**自动**停用/自动启用时真实发出一条飞书消息。管理员手动停用/启用**不发**。发送必须异步，绝不阻塞转发热路径。

**Blocked by:** 01, 02（需要配置存储与发送能力）

**Status:** ready-for-agent

- [ ] `src/notification/notify.rs`：`spawn_provider_transition(db, provider_id, transition)` —— **同步入口**，内部 `tokio::spawn`。理由：连续失败熔断发生在转发热路径上，绝不能因飞书 HTTP 调用而阻塞
- [ ] `Transition` 枚举：`QuotaDisabled{label}` / `QuotaRecovered{label}` / `FailureDisabled{consecutive}` / `FailureRecovered`
- [ ] 异步任务内部：读通知配置（**单行查询、无缓存** → 无失效 bug）→ 未配置或已停用则直接返回 → 按 `provider_id` 查供应商名（已删除则跳过）→ 拼文案 → 取 token → 发消息 → 回写 `last_error` / `last_sent_at`
- [ ] 在 `src/availability.rs` 四个动作**确有转换**的分支（即原本返回 `Ok(true)` 处）调用：`disable_for_quota` / `recover_quota` / `disable_for_failures` / `recover_probe`。触发点收敛在动作内部而非调用方，未来新增调用方不会漏发
- [ ] **不改这四个函数的签名**——`disable_for_failures` / `on_forward_failure` 只有 `request_id` 没有供应商名，由异步任务按 id 查
- [ ] 覆盖 5 类自动转换：额度门控自动停用、额度门控自动恢复、边界实测停用、边界实测恢复、连续失败熔断、自动恢复探测启用
- [ ] `disable_manual` / `enable_manual` **不触发**
- [ ] 文案（纯文本）：标题 `【供应商自动停用】` / `【供应商自动启用】`；原因 `订阅额度耗尽` / `余额耗尽` / `连续失败 {n} 次` / `订阅额度恢复` / `余额恢复` / `自动恢复探测成功`；时间按**设置表时区**（缺省 `Asia/Shanghai`）格式化
- [ ] **不重试、不排队、不做发送失败补偿**；失败只写 `last_error` + tracing 日志
- [ ] 单测：四类转换的文案渲染（标题/原因/时间格式）；主流程 `await` 直调 mock（不 spawn，确定性）——成功写 `last_sent_at`、失败写 `last_error`
- [ ] 集成测试：配置通知 + mock，走既有用量刷新路径真实触发一次额度门控停用，断言 mock 收到自动停用消息（**有界轮询等待**，非固定 sleep）；再验证「手动停用不发」——手动停用后触发一次自动转换，断言 mock **恰好只收到 1 条**
