# 供应商、用量与通知

供应商（provider）/ 供应商模型（provider_model）/ 虚拟模型（virtual_model）三层的领域不变量见 ADR-0020，本文记录跨层机制。

## 供应商用量查询（ADR-0009）

`GET /api/providers/{id}/usage?refresh=1`（代码在 `src/usage/`）。对 `extra.usage=true` 的供应商按 base_url host（火山/阶跃再看 path 区分订阅 Plan 与按量账户）分发到各厂商 fetcher（API key 直查 / Copilot OAuth / 火山与阿里 AK/SK 签名 / CookieCloud cookie 系 / OpenDesign Control Key + 工作区双读），归一化为订阅制窗口（5h/周/月，厂商不提供的窗口 `available=false`）或按量余额条目。

成功结果写**数据库缓存**（`provider_usage_cache` 表）：10 分钟内直出，过期/缺失才真实抓取并重新落库；更新/删除供应商时失效；`?refresh=1` 强制重取。详情页内嵌「用量信息」卡片（进度条按剩余百分比着色）。

对外只读入口（Bearer API Key 鉴权，复用 `/v1` 中间件，见 `src/routes/v1_providers.rs`）：`GET /v1/providers` 列出供应商 ID/名称/开关/计费模式/用量支持标记（不暴露 api_key、base_url 等敏感配置），`GET /v1/providers/{id}/usage`（支持 `?refresh=1`）与管理后台用量接口共享同一核心（`providers::provider_usage_response`），行为与错误口径一致。

## 用量自动刷新与额度门控（ADR-0010）

内置定时任务 `usage_refresh`（`@every 5m`，种子行见 `src/cron/seed.rs`）刷新全部用量供应商（**不含 enable 过滤**，停用的也持续监测）并落库，随后执行门控：

- 订阅制供应商额度耗尽（任一厂商已提供的窗口剩余为 0）→ 自动停用该 Provider 及名下全部虚拟模型子模型；恢复后自动启用。
- 窗口剩余百分比落在 (0,1) 边界区 → 先发最小测试请求实测：失败同样停用、成功保持/解除停用。恢复有双通道：窗口余量回升离开边界区，或实测成功。
- 实现：`src/usage/persist.rs::apply_usage_gate` / `probe_boundary_providers`。实测请求与模型弹窗测速同口径记入 request 表（`api_key_name='test'`），但不计入数据面板统计（见 `stats.md`）。

连续失败熔断与自动恢复探测见 ADR-0012（每小时 handler 业务体在 `src/failure_recovery.rs`）。

## 飞书机器人消息通知（ADR-0023）

供应商发生**自动**停用/自动启用（额度门控停用/恢复、边界实测停用/恢复、连续失败熔断、自动恢复探测）时异步发一条纯文本飞书消息；管理员手动启停不发。

配置为单行 `notification_channel` 表（飞书应用凭据整段加密落库、接口掩码返回）。设置页「消息通知」弹窗支持手填 App ID/Secret、扫码一键创建（OAuth 2.0 Device Authorization Grant，前端渲染二维码 + 后端会话轮询）与测试发送。

发送尽力而为：不重试、不阻塞转发热路径，只记录最近一次失败原因。实现取舍（为何不引入官方 SDK、轮询状态机易错点）见 ADR-0023。

## 模型测速页（ADR-0027）

侧边栏「观测」分组 `/speed-test`，对供应商模型做 TPS 基准。

**候选集** = 供应商 `enable=true` 且 `disabled_reason` 为空（`provider_model` 无 enable 字段，模型层不再二次判定）。过滤为供应商/虚拟模型多选 + 关键词（供应商名 ∪ 上游模型名 ∪ 关联虚拟模型 `display_id`，子串忽略大小写），条件写进 URL query。`GET /api/speed-test/candidates` 与启动共用 `speedtest::candidate::resolve` 单一事实源：候选接口返回 `models[]` 完整清单，筛选一变化即展示会测哪些模型，不必等跑过测速；运行响应带回 `providerIds`/`vmIds`/`keyword` 快照，前端据此判断结果是否属于当前筛选。

**请求**：流式 + 内置长文题面 + `max_tokens` 取模型登记值（不夹取，未登记兜底 2048），经 `build_upstream_call` 四协议统一、`PumpSource::feed_for_metrics` 复用转发事件解析、`RequestRecord::tps_value` 与 `request.tps` 同口径。落库仍带 `api_key_name='test'`（故不入统计、不计熔断），`request_id` 前缀 `speedtest-`。

**运行模型**：单实例 + 可取消，**无并发上限**（所有候选同时开测、不做同供应商串行），不设单模型硬超时（耗时由上游客户端自身超时兜底）。开始即写全部 `pending` 占位行并逐行终态化；四态 `running/completed/cancelled/interrupted`，启动时残留 running 改判 interrupted。结果落 `speed_test_run`/`speed_test_result`，只留最近一次运行（新运行即删旧结果），结果行冗余供应商名等快照字段。

**前端**：2s 轮询 `GET /api/speed-test/run`，切页面回来即恢复。供应商详情的旧弹窗保留并改名「连通性测试（时延）」。

## 模型目录（ADR-0001 / ADR-0025）

`models_refresh` 每天 04:00 从 models.dev 拉取全量模型目录并原子写入外置文件；读取侧优先外置文件（带 mtime 缓存），缺失时回退二进制内嵌的快照。
