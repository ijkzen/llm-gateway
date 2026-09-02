# SPEC: LB 熔断与失败复查

Label: `ready-for-agent` · Source: [REQUIREMENTS.md](./REQUIREMENTS.md) · 2026-09-03

## Problem Statement

管理员配置的虚拟模型在转发时依赖 LB 排序挑选成员，但排序依据的是最长 10 分钟新鲜的用量缓存。当缓存过期、上游实际已无可用额度时，请求仍会先打到该成员并失败，才触发降级——产生无谓的失败延迟。另一种情况是上游服务持续异常（不是额度问题），现有机制只会逐请求降级，坏供应商永远留在候选列表里持续制造失败。

## Solution

两级自动处理 + 一级手动恢复：

1. 成员请求失败后，后台异步对支持用量查询的供应商做实时用量核验——确认耗尽立即走现有额度门控禁用（额度恢复后 usage_refresh 自动恢复），把「缓存过期导致的降级」在本请求之后就切断。
2. 所有失败（含不可重试的 4xx）计入供应商粒度的内存连续失败计数，达到可配置阈值（默认 5）时把供应商连同名下全部虚拟模型条目一起禁用，并打上 `failure_disabled` 标记——这种禁用不会被 usage_refresh 自动恢复，只能管理员手动启用解除。
3. 供应商任一请求成功即清零计数，避免把偶发抖动累积成误禁。

## User Stories

1. As a 网关调用方, I want 已耗尽额度的供应商在首次失败后被立即复核并禁用, so that 后续请求不再浪费一次注定失败的尝试。
2. As a 网关调用方, I want 持续故障的供应商在连续失败达到阈值后被移出候选, so that 我的请求尽量落在健康的上游上。
3. As a 网关调用方, I want 偶发一次失败不会禁用供应商, so that 正常的上游抖动不会造成服务面收窄。
4. As a 管理员, I want 连续失败阈值可配置（默认 5）, so that 我能按上游稳定性偏好调校灵敏度。
5. As a 管理员, I want 连续失败禁用的供应商不被 usage_refresh 自动恢复, so that 一个仍在上游持续报错 5xx 的供应商不会被定时任务悄悄放回来。
6. As a 管理员, I want 手动启用供应商即解除连续失败禁用并清零计数, so that 处理完上游问题后有一条明确的恢复路径。
7. As a 管理员, I want 额度耗尽导致的禁用仍然保留自动恢复, so that 额度型停用无需人工盯梢（与连续失败禁用区分开）。
8. As a 管理员, I want 连续失败被禁用的供应商用量卡片仍在刷新, so that 我能在后台看到它的实时额度以判断是否手动恢复。
9. As a 管理员, I want 禁用动作有带 request_id 的日志可查, so that 事后能还原「为什么它被禁了」。
10. As a 管理员, I want 阈值设置项修改后立即生效, so that 调整灵敏度不需要重启服务。

## Implementation Decisions

- **计数状态**：进程内存 `HashMap<provider_id, u32>`，新增独立小结构（仿 LbState 的 `Arc<Mutex<…>>` 模式）挂入 AppState；provider 粒度，不区分模型；任一成功请求清零；不持久化。
- **失败口径**：failover 循环的 4 个失败分支（密钥解密/请求构造/传输错误/可重试状态码 408|429|500|502|503|529）**与**非可重试 4xx 终态返回路径均计数；4xx 不参与降级的现有行为不变。
- **失败复查**：失败后 spawn 后台任务；仅对 `extra.usage=true` 的供应商执行 `fetch_and_store` 强制实时抓取并写 DB 缓存；判定复用现有额度门控谓词——耗尽则执行与 `apply_usage_gate` 相同的禁用动作，充足则不动作；不支持用量查询的供应商直接跳过。
- **复查节流**：同一 Mutex 条目维护 in-flight 标记 + 上次触发时间，进行中或短时间窗（60 秒）内不重复触发；不引入新依赖。
- **连续失败禁用**：计数达到阈值时——provider `enable=false`、名下全部虚拟模型条目按现有级联语义停用（`cascade_disabled=true`）、`failure_disabled=true`、记 warn 日志（含 request_id、provider_id、连续失败次数）；并发下多个请求同时触达阈值只执行一次禁用。
- **Schema**：Migration 17 给 provider 表新增 `failure_disabled` 布尔列（默认 false）；沿用启动时 column_exists 检测的历史库兼容模式。
- **设置项**：`max_consecutive_failures`，Int 类型，默认 5；走现有 AppSettings 的种子/内存缓存/热更新模式；校验为正整数。
- **usage_refresh 交互**：刷新范围不变（不看 enable，failure_disabled 的照常刷新以维持展示）；恢复分支额外跳过 `failure_disabled=true` 的供应商。
- **手动启用供应商**：清除 `failure_disabled`、清零内存计数；子模型仍只恢复 `cascade_disabled=true` 的条目（现有语义不变）。
- **术语**：领域词汇见 CONTEXT.md「可用性域」（额度门控禁用 / 连续失败禁用 / 连续失败计数 / 失败复查）。

## Testing Decisions

- 好的测试只断言外部可见行为：HTTP 响应、DB 中 provider/虚拟模型条目的 enable 与 failure_disabled 状态、设置项校验行为；不断言内部 Mutex 结构或函数调用次数。
- **主 seam（HTTP 集成）**：新集成测试文件，复用 `build_authed_app` + 本地 mock 上游（令成员稳定失败/成功）+ `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 用量重定向（驱动复查判定）。覆盖：连续失败达阈值→禁用+标记+子模型级联；成功清零；refresh_all_usage 不恢复 failure_disabled 但恢复额度门控禁用；手动启用清除标记；复查耗尽→禁用。先例：proxy_integration、provider_quota_gate_integration。
- **辅助 seam（单元）**：节流判定与阈值比较的纯逻辑测试。先例：usage 模块单测。
- **辅助 seam（设置集成）**：默认种子值、PUT 校验（拒绝 0/负数/非整数）、修改后运行时读取到新值。先例：settings_integration。

## Out of Scope

- 半开探测/自动恢复试探、指数退避等熔断器高级形态。
- 按模型粒度的计数或禁用。
- 禁用通知/告警推送（仅日志）。
- 失败计数持久化与跨重启保留。
- 前端专属 UI（设置页若为动态列出则自然出现新设置项；供应商列表的禁用来源标注不做）。

## Further Notes

- 生产 schema_migrations 残留废弃号段，新迁移编号从 17 起且须做 column_exists 兼容（历史教训，见 AGENTS.md 迁移注意事项）。
- 复查抓取失败（上游用量接口异常）按无数据处理：不触发额度禁用、不改计数——计数由转发失败路径负责，两个路径职责分离。
- usage_refresh 每 5 分钟一轮，与失败复查的实时性互补：复查负责「立刻切断」，refresh 负责「额度恢复后自动放回」。
