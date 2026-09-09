# 04 · proxy 上游传输与探活审查

Type: task
Status: claimed
Blocked by: 01

## Question

对 proxy 上游传输域做全量审查：`upstream.rs` / `pool.rs` / `probe.rs`（test_model/probe_provider/ProbeFailure）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：连接池归还/释放竞态、空闲超时、TTFT 起点口径、探活与真实请求的并发关系；
- 实现简洁：客户端构造、TLS 配置克隆、错误映射是否有收敛空间（09-08 审计已整改项不复查）；
- 测试覆盖：upstream_pool 集成测试之外缺什么（超时/中断/坏上游场景）；
- 模块间调用：probe 被 usage 边界探活与 cron failure_recovery 双消费，接口是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/04-proxy-upstream-transport.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/04-proxy-upstream-transport.md`——13 条全 P3，无 P1/P2，分列于四轴 + 性能轮。传输层是本图迄今最干净的模块（09-08 四维审计的 P1-P6 在此域的整改均已落地且无回归）。

- **逻辑正确性**（04-01/02/03）：复用连接重试以 `timing.total_ms()==0` 隐式判定「复用」与意图脱节（回环新连接 0ms 也触发重试）+ 陈旧连接重发的双重计费窗口无注释固化（默认解=显式 `was_reused` 布尔 + 注释）；IPv6 字面量主机不可用且报错误导（已实证 http 1.4.2 `Uri::host()` 保留方括号，默认解=parse_url 显式拒绝+单测）；probe duration 与 metrics 非流式分母两处注释与实现不符（口径本身与真实流量同公式，纯注释漂移）。
- **实现简洁**（04-04/05/06）：`UpstreamCall.stream` 与 `UpstreamReply.connect_done_at_ms` 死字段（赋值后无读者，删字段级联清理）；探活前奏（取最小模型+解密 key+空 key 判定）在 probe_provider/failure_recovery/路由三处重复（语义现逐行一致，抽共享 helper）；test_model 成功行 request_id 与上行调用期 id 不一致（失败行一致）——排障断链。
- **测试覆盖**（04-07/08/09/10）：陈旧连接重试零测试（04-01 行为唯一无锁定路径）；四组超时为模块常量不可注入 → 超时路径零覆盖需测试缝；建连失败族与 CONNECT 代理失败族（407/垃圾/超长/带 userinfo）零测试；池并发/隔离/cleaner 直收路径缺测（串行单 key 仅 5 例）。
- **模块间调用**（04-11/12）：failure_recovery `probe_gate` 每小时真抓厂商用量（`fetch_and_store`）——usage_refresh 每 5 分钟已维护缓存，应先 `read_usage_cache` 缺失才抓（实现细节归口 06 票）；手动测速路由无超时/无取消（最坏 ~4 分钟挂起），cron 双 handler 有 try_lock 自愈、路由裸奔。
- **性能轮**（04-13）：每 host 并发连接无上限 + 突发滞留 600s——观察级；归还/释放无竞态、TTFT 起点口径、单连接不共享等关键面均核验无问题（见 findings「已核验无问题区」，含 http 1.4.2 源码实证）。

**需拍板问题**：无。本票无 P1/P2、无行为口径分裂的决策点——探活与真实流量同口径超时（120s×2）是既有设计（边界探活失败=停用语义已在 ADR-0010 拍板，短超时会误杀慢供应商），其余条目默认解明确（显式布尔、删死字段、抽 helper、缓存优先、注释修正、补测试缝），全部为图后实施批 P3 项，按清单模式直接落清单。若实施批评估中认为 04-12（测速路由取消/超时）值得脱离批次先做，届时再单独拍板。

Status: resolved
