# FINDINGS · 04 proxy 上游传输与探活审查（2026-09-10）

范围：`upstream.rs`（连接建立/代理隧道/超时/单次调用）、`pool.rs`（连接池/归还/清理）、`probe.rs`（test_model/probe_provider/ProbeFailure）+ 域内测试（`tests/upstream_pool_integration.rs` 5 例、`tests/provider_models_test_integration.rs` 测速端点、`provider_boundary_probe_integration.rs`/`provider_failure_recovery_integration.rs` 对 probe 的消费）。方法：三文件全量逐行通读 + 消费方交叉核对（lib.rs 两个 cron handler、usage/persist.rs `probe_boundary_providers`、failure_recovery.rs、routes/provider_models.rs test 端点、dispatch/metrics 记账口径——后两者属 03 票域，本票只锚接口与口径引用）+ 测试覆盖盘点 + 性能/内存专项。清单模式：不改代码，条目供图后统一排期。**本票无 P1/P2、无行为口径分裂的决策点**（探活超时与真实流量同口径是既有设计，边界探活失败=停用语义已在 ADR-0010 拍板），全部条目为 P3 级、默认解明确，直接进清单，不需拍板。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 04-01 | P3【已修复 2026-09-10】 | 逻辑正确性/重试 | 复用连接重试以 `timing.total_ms()==0` 隐式判定「复用」，意图与表达脱节（回环新连接 0ms 也会重试）；陈旧连接「上游已受理、响应前断连」的重发存在双重计费窗口且无注释固化权衡 |
| 04-02 | P3【已修复 2026-09-10】 | 逻辑正确性/边界 | IPv6 字面量主机（`[::1]`）未处理：http crate `Uri::host()` 返回含方括号串（实证 http 1.4.2 源码，Cargo.lock 锁定版本），`lookup_host`/`ServerName::try_from` 必败且报错误导；`parse_url` 零单测、无显式拒绝 |
| 04-03 | P3【已修复 2026-09-10】 | 口径/注释漂移 | probe 成功 duration 注释「上游响应开始到读完，排除 TTFT」与代码（`end − start_at_ms`，含建连与首字节等待）不符；metrics.rs 非流式分母注释「请求发出→读完」同样只对复用连接成立 |
| 04-04 | P3【已修复 2026-09-10】 | 简洁/死字段 | `UpstreamCall.stream`（calls.rs:84 赋值后无读者）与 `UpstreamReply.connect_done_at_ms`（写 5 处读 0 处）为死字段；`StreamMetrics.connect_done_at` 命名即 TTFT 起点，名不副实 |
| 04-05 | P3【已修复 2026-09-10】 | 简洁/重复前奏 | 「取最小模型 + 解密 key + 空 key 判定」前奏在 probe_provider / failure_recovery / 路由 test 端点三处重复（语义现一致，漂移风险），抽共享 helper 收敛 |
| 04-06 | P3【已修复 2026-09-10】 | 可观测性/记账 | test_model 成功行 `request_id` 与上游调用期 id 不一致（两处 `Uuid::new_v4()`）——request 表成功行 ↔ 调用日志关联断链（失败行一致） |
| 04-07 | P3【已修复 2026-09-10】 | 测试覆盖 | 陈旧连接「空闲后对端静默关闭 → 发送失败 → 重试一次」（04-01 行为）零测试——第二次请求应新建连接成功且连接数+1 |
| 04-08 | P3【已修复 2026-09-10】 | 测试覆盖/测试缝 | 四组超时（CONNECT/TLS/HEADER/NON_STREAM_BODY）为模块常量（upstream.rs:43-49）不可注入——silent-upstream 超时路径零覆盖，需超时可注入测试缝 |
| 04-09 | P3【已修复 2026-09-10】 | 测试覆盖 | 建连失败族（DNS 失败/拒连/多地址首败次成/空地址）与 CONNECT 代理失败族（407/垃圾响应/超长/带 userinfo 拒绝）零测试；`parse_url` 无任何单测 |
| 04-10 | P3【已修复 2026-09-10】 | 测试覆盖 | 池并发与隔离缺测：同 key N 并发应 N 连接且全部归还、http/https 同 host:port 的 scheme 隔离、cleaner 60s 直收路径（现仅惰性 checkout 路径有测） |
| 04-11 | P3【已拍板保持现状 2026-09-10】 | 模块间调用 | failure_recovery `probe_gate` 每小时对每家「failure 停用+用量开启」供应商真实抓取用量（`fetch_and_store`）——usage_refresh 每 5 分钟已维护缓存（≤5min 新），应先 `read_usage_cache` 缺失才抓；用量 API 瞬时故障会把恢复推迟整小时 |
| 04-12 | P3【已拍板保持现状 2026-09-10】 | 健壮/运维 UX | 手动测速路由最坏 ~4 分钟挂起（连接 20s+头 120s+体 120s）且客户端断开不取消——cron 双 handler 有 `try_lock` 自愈（lib.rs:171/203），路由层无任何守卫；UI 按钮已 loading 防抖（ProviderModelDetailDialog） |
| 04-13 | P3【已登记观察 2026-09-10】 | 性能轮 | 每 host 并发连接无上限：HTTP/1.1 单连接串行，同 host 并发请求各自新建连接+驱动任务，突发后空闲连接滞留至 600s 空闲超时——当前并发温和属观察级 |

## 各条证据

### 04-01 复用连接重试：条件隐式 + 双重计费窗口无注释（P3，逻辑正确性/重试）【已修复 2026-09-10】

upstream.rs:488 `Err(UpstreamError::Request(_)) if attempt == 0 && timing.total_ms() == 0`——「仅复用连接可重试一次」的意图用「建连计时为 0」隐式表达：fresh 路径在 upstream.rs:450-451 赋值 `timing = measured`，复用路径（:462-466）只赋两个时间戳、`timing` 保持 default 零值。回环/本地上游的建连总耗时（TCP+TLS）可能整毫秒截断为 0，此时**新连接首发失败也会触发重建重发**——本意外的额外重试；而真实网络 fresh 计时必 ≥1ms，故该分支实际几乎只服务复用场景，「0ms」只是碰巧成立的代理判定。影响面小，但条件表达与注释意图（:489「复用连接可能已被对端静默关闭」）不符。修复默认解：`let mut was_reused` 显式布尔替代计时判定。

另：重试本身存在「上游已受理请求、在响应前断连 → hyper 返回 Request Err → 重发同一请求」的双重计费窗口（上游侧首次可能已开始生成），本层无法区分「发送前断」与「已受理后断」。这是复用池重试的内在权衡、非 bug，但当前零注释——修复时以注释固化（或对非幂等的 LLM 请求评估保留重试的必要性，保守可仅对发送阶段错误重试——hyper 错误类型无法区分，注释即可）。

### 04-02 IPv6 字面量主机未处理（P3，逻辑正确性/边界）【已修复 2026-09-10】

`parse_url`（upstream.rs:133-152）对 `http://[::1]:8080` 类 URL：http crate `Uri::host()` 对 IPv6 字面量返回**含方括号**的串（实证：Cargo.lock 锁定 http 1.4.2，`uri/authority.rs` 的 `fn host` 切片 `&host_port[0..i+1]` 含两个括号）→ `tokio::net::lookup_host(("[::1]", port))` 以带括号串做 getaddrinfo 必败（报「DNS 解析失败」误导）；https 路径 `ServerName::try_from("[::1]")` 报「TLS 主机名无效」。IPv6 无任何路径可用，错误信息指向 DNS/主机名而非真实原因。代理地址解析（connect_via_proxy 的 `rsplit_once(':')`，upstream.rs:261-269）对 IPv6 代理同理。默认解：`parse_url` 显式识别并拒绝 IPv6（清晰中文错误）+ 单测锁定；真支持需去括号 + rustls `ServerName::IpAddress` 分支，成本不值。

### 04-03 口径注释漂移（P3，口径/注释）【已修复 2026-09-10】

- probe.rs:16-18 文档：「`duration_ms`：本次请求耗时（上游响应开始到读完，与 request 表 `output_tokens_time` 同口径，**排除 TTFT**）」——代码（probe.rs:130-131）= `end_time − reply.start_at_ms`，而 `start_at_ms` 口径为「新建连接=TCP 建连开始时刻，复用连接=请求发出时刻」（upstream.rs:426-429）。实际含建连 + 首字节等待（TTFT）全段，「排除 TTFT」「响应开始到读完」两处描述均与实现不符。
- metrics.rs:93-96 tps 注释：非流式分母括注「end_time − ttft_start_ms（**请求发出** → 读完）」——同款偏差：只对复用连接成立，fresh 连接起点是建连开始（:93-94「起点=建连开始或请求发出」正确，:95 括注「请求发出」以偏概全）。

值得说明：**口径本身与真实流量一致**（dispatch.rs:204/300 非流式成功行同公式 `end − start_at_ms` + `ttft_start_ms = start_at_ms`），probe 记账可比性正确，纯注释漂移。默认解：两处注释按真义改写（含建连与上游处理全程、不含网关前置）。

### 04-04 死字段与命名（P3，简洁）【已修复 2026-09-10】

- `UpstreamCall.stream`（upstream.rs:416-417）：calls.rs:84 赋值 `stream: client_stream || protocol == Responses` 后**无任何读者**（全仓 grep 实证）——「流式不设体超时」由调用方选择 `read_body`（有超时）或直接消费 PooledBody 帧流（无超时）实现，与字段无关。默认解：删字段 + calls.rs:84 赋值（`client_stream` 参数保留，gemini `generate_action` 仍需）。
- `UpstreamReply.connect_done_at_ms`（upstream.rs:429）：仅本文件写入（:462/466/495），无读者（StreamMetrics 的 `connect_done_at` 是另一个字段）。文档注释（:427-428）声称「复用连接时作为 TTFT 起点的近似」——复用路径实际赋 `now`（:466）与 `start_at_ms` 相同，语义冗余。默认解：删字段并同步头注释（TTFT 起点即 `start_at_ms` 单一口径）。
- `StreamMetrics.connect_done_at`（metrics.rs:30-31 命名）：字段语义是「TTFT 起点」（注释自述「新建连接=建连开始，复用连接=请求发出」），名不副实（03 票域文件，同批顺手改名 `ttft_start_at`）。

### 04-05 探活前奏三处重复（P3，简洁/收敛）【已修复 2026-09-10】

「按 `model_id` 升序取最小模型 + 解密 api_key + 空 key 判定」同一前奏逐行重复于：

- probe.rs:167-186（`probe_provider`：查询失败/无模型/空 key/解密失败 → 各自 `Skipped` 文案）
- failure_recovery.rs:47-93（`recover_failure_disabled`：同款查询/解密，但每步独立 warn 日志点名供应商）
- routes/provider_models.rs:823-837（test 端点：仅解密段，404 语义需要先查 provider/model）

probe_provider 与 failure_recovery 两段今天语义逐行一致（同排序、同 `.one()`、同空 key 判定），但 failure_recovery 因「每阶段要日志」绕开 probe_provider 直调 `test_model`（failure_recovery.rs:95），probe_provider 的 Skipped/Failed 分类对它形同虚设——`probe_provider` 事实上只有一个消费者（persist.rs:258）。默认解（图后实施批评估形态）：抽共享前奏 helper（返回「模型+解密 key」或阶段化失败原因），probe_provider 与 failure_recovery 复用，日志留在各自调用层；路由段保持现状（404 语义前置需要）或转调 helper。

### 04-06 test_model 成功行 request_id 断链（P3，可观测性/记账）【已修复 2026-09-10】

probe.rs:34 生成 `request_id`（"test-{uuid}"）传入 `build_upstream_call`（上行调用侧身份）；失败路径（:54/76）`record_failure` 复用同一 id ✓；**成功路径（:133）重新生成一个 uuid 落行**——同一次测试请求，request 表成功行 id ≠ 调用期 id。正式流量两侧同 id（forward.rs 同一变量贯穿），排障时可把 request 行 ↔ 入口日志 ↔ 上游调用对上；test 成功行对不上。默认解：成功行复用 `request_id`（删 :133 二次生成）。无唯一性冲突（request 表 id 非主键约束，且行 id 本就按请求唯一）。

### 04-07 陈旧连接重试零测试（P3，测试覆盖）【已修复 2026-09-10】

upstream.rs:488-505 的「复用连接发送失败 → 丢弃并新建重试一次」是池化连接唯一自愈路径，`upstream_pool_integration.rs` 无对应场景：现测的三例（复用/空闲超时/Connection: close）都走健康连接。缺的 mock：首个响应正常返回后，服务端**保持连接空闲若干毫秒再主动 close**（模拟上游空闲超时回收），第二次 `call` 需命中「checkout 时 `is_closed()` 未及翻转 → 发送失败 → 重试新建」并断言第二次成功 + 连接计数为 2。注意：checkout 已有 `is_closed()` 惰性过滤（pool.rs:162），测试需制造竞态窗口（close 后立即发第二次请求），若无法稳定命中该窗口，也可直接对 `call()` 注入「预置一条已关闭的 sender」做确定性单测（sender 需真连接——可先 checkout 健康连接、服务端 close、sleep 后再 call）。

### 04-08 超时常量不可注入（P3，测试覆盖/测试缝）【已修复 2026-09-10】

upstream.rs:43-49 四组超时为模块级 `const`：`CONNECT_TIMEOUT`(10s)/`TLS_HANDSHAKE_TIMEOUT`(10s)/`HEADER_TIMEOUT`(120s)/`NON_STREAM_BODY_TIMEOUT`(120s)。后果：silent-upstream（accept 后不响应）/半写体/握手悬挂等超时路径在测试中无法缩短时间到可承受范围，全仓零覆盖（超时变体 `UpstreamError::Timeout` 的映射/文案/落库行为无测试）。默认解：超时经 `UpstreamPool`（或 `call` 参数）注入、生产值不变，测试传毫秒级值补 silent-upstream 与 body 悬挂两例。

### 04-09 建连失败族与 CONNECT 失败族零测试（P3，测试覆盖）【已修复 2026-09-10】

`upstream_pool_integration.rs` 只测 happy path + `Connection: close` + 代理连通/隔离：

- 直连失败族零覆盖：DNS 解析失败、连接拒绝（close 端口）、多地址首败次成（mock 一个拒绝地址 + 一个可用地址的解析顺序难控——可降级为对 `connect_stream` 做单元级测试或跳过）、空地址。错误映射（`UpstreamError::Connect` 文案含 host/addr）与 failover 触发（Err(Connect) 冒泡给成员循环）无直接断言。
- CONNECT 代理失败族零覆盖：代理返回 407/500、代理拒绝后关闭连接、响应超长（>4096 无 `\r\n\r\n`，upstream.rs:367-372 上限分支）、带 `user:pass@` 的代理地址（:256-260 防御分支）、非 `http://` 前缀（:252）。这些防御分支都是死代码风险点。
- `parse_url`（upstream.rs:133-152）零单测：默认端口推断（443/80）、空 path → `/`、query 保留、缺 host 报错、IPv6（04-02）——全无锁定，04-02 的拒绝策略应在此落单测。

### 04-10 池并发与隔离缺测（P3，测试覆盖）【已修复 2026-09-10】

现 5 例全串行、单 key。缺：同 key N 并发（`JoinSet` 同时 N 个 call → 连接数应 = N，全部归还后第 N+1 次应复用不新建）；scheme 隔离（同 host:port 下 http 与 https 不可直接 mock——https 需本地 TLS server，成本高，可记为可选）；cleaner 直收路径（60s 扫描回收——现测例靠 checkout 惰性过滤（pool.rs:153-166），`spawn_cleaner` 的 `list.retain` 直收分支（:133-137）无直接测试；若测试缝把扫描间隔也做成可注入可低成本补）。协议层面结论先行：归还/释放无竞态（`settled` 一次性 + 归还绑定 body 生命周期 + 客户端提前 drop 即连关，pool.rs:48-62），N 并发各自持有独立 sender，单连接从不被双请求共享（busy 连接不进池）——此结论值得一个并发测试钉死，防未来重构回归。

### 04-11 probe_gate 每小时真抓用量（P3，模块间调用）【已拍板保持现状 2026-09-10】

failure_recovery.rs:142-163 `probe_gate`：对 `failure` 停用且用量开启的供应商调用 `fetch_and_store`（:146）——**真实抓取厂商用量 API**（部分厂商是登录级多跳流程，如 CookieCloud/账号密码系），每小时每家一次。而 usage_refresh 每 5 分钟全量刷新并落库（lib.rs:164-183），恢复探测运行时数据库缓存至多 5 分钟新——`read_usage_cache`（10 分钟新鲜度判定，persist.rs:28）几乎必然命中。后果：无谓的厂商 API 调用量与故障面（用量 API 瞬时故障 → `Blocked` → 本可恢复的供应商恢复推迟一整小时）。默认解：`probe_gate` 先 `read_usage_cache`，过期/缺失（None）才回落 `fetch_and_store`。接口观察归口：抓取层实现细节入 06 票范围，此处仅记消费方接口适配问题。

### 04-12 手动测速路由无超时/无取消（P3，健壮/运维 UX）【已拍板保持现状 2026-09-10】

routes/provider_models.rs:798-857 `test_provider_model`：直调 `proxy::test_model`（:839），无任何超时包装。test_model 内部最坏时长 = 建连 20s（双 10s）+ `HEADER_TIMEOUT` 120s + `read_body` `NON_STREAM_BODY_TIMEOUT` 120s ≈ **260s+**（DNS 未计）；客户端断开后 axum handler 照常跑完（无取消传播）。对照：cron 侧双 handler 都有 `try_lock` 防重叠自愈（lib.rs:171/203），LB 侧无此形态。UI 按钮已有 pending 防抖（ProviderModelDetailDialog `useTestProviderModel` isPending 禁用），服务端重复并发概率低，但坏上游场景 operator 点击测试 = 页面级 4 分钟假死且无法中止。默认解（保持探活与真实流量同口径超时——边界探活失败=停用语义（ADR-0010）要求探活不被短超时误杀，**不建议**为探活单设短超时）：可选在实施批评估路由层「断开即取消」（axum 需把 handler 挂到连接生命周期，成本中等），或保持现状并在按钮侧提示最长等待时间——倾向后者，成本最低且语义一致。

### 04-13 每 host 并发连接无上限（P3，性能轮）【已登记观察 2026-09-10】

池按 key 存**空闲**连接，busy 连接不进池；hyper HTTP/1.1 客户端单连接不并发请求（`SendRequest` 排队语义），同 host 并发 N 个请求 = 同时 N 条 TCP+TLS 连接 + N 个 spawn 驱动任务（upstream.rs:455/499）。突发结束后连接全部归还，滞留至 600s 空闲超时（cleaner 60s 扫描粒度，pool.rs:122-143），期间每条连接占用一个 tokio 任务与内核资源。当前网关并发形态（管理面单用户 + 少量客户端，各 host 峰值并发个位数）下无实际影响；若未来出现同一上游大量并发，需按 key 限制在途连接数或在池上排队复用。**观察级条目，无默认动作**，仅在性能放大时回看。另两处微观察并入：DNS `lookup_host` 裸调无超时（upstream.rs:174，受系统 resolver 约束，非代码可控）；多地址串行逐个 10s（:184-197）——坏上游建连最坏 N×10s+10s TLS 后才冒泡 failover，IPv6 首地址不可达场景会放大此等待。

## 已核验无问题区（避免后续票重复审查）

- **归还/释放无竞态**：`settled` 一次性标记（pool.rs:50-62）+ 归还绑定 body 生命周期（poll_frame EOF/Err 才 settle）+ 客户端提前 drop → sender drop → hyper conn 驱动任务收尾关闭底层连接（pool.rs:3-6 文档与 hyper 语义一致）；`release` 持锁 push、`checkout` 持锁 pop，同一连接绝不被双请求共享（busy 连接不在池中）。
- **TTFT 起点口径**：`start_at_ms` = 建连开始（新）/ 请求发出（复用）——与 AGENTS.md、entity/request.rs 文档、metrics.rs ttft 公式逐条一致；fresh 路径建连耗时并入 ttft 无独立 network_latency 字段（文档一致）；响应头 `HEADER_TIMEOUT` 只守到首字节、流式体不设总超时——长推理流（>120s 首内容）不受影响（上游先回 200+头再推事件）。
- **探活与真实请求并发关系**：probe/test 与正式流量共享同一 `UpstreamPool`；池只含空闲连接，探活不阻塞、不被真实请求阻塞（miss 即新建），无 head-of-line 问题；探活记录写入与正式流量同管线（同一 `RequestRecord.insert` → P4 单写者），行以 `virtual_model_id=0`/`api_key_name=test` 标识（既有口径，非本票范围）。
- **单次调用构造**：保留名头（Host/Content-Type/accept/Content-Length）由发送端唯一写入（upstream.rs:521-527），组装层保证 `call.headers` 不含同名项——无重复头风险；body 全量缓冲 + Content-Length（无 chunked 请求体）。
- **TLS**：`tls_config()` OnceLock + Arc 克隆（09-08 P6 已修，upstream.rs:118-130），SNI 按次 `connect(server_name, …)` 传入、共享配置无状态泄漏；webpki 根仅公共 CA——自签/私有 CA 上游不可达属运营约束（无开关），非回归。
- **CONNECT 代理**：仅 http:// 无认证代理（防御分支拒绝 userinfo）；连接池按「代理地址|目标」隔离（upstream.rs:442-445）；CONNECT 读响应到 `\r\n\r\n` 为止、剩余字节留在流中给 TLS（upstream.rs:361-395）——无字节错位。
- **池后台任务**：cleaner 惰性+直收双路径都正确（checkout 过期即弃、60s 扫描 retain）；每 `UpstreamPool::new` 一个 cleaner（tokio interval 首 tick 立即执行一次，无影响）；生产仅 AppState 单池。
- **超时语义**：Timeoute 变体不触发重试（仅 Request 错误重试，upstream.rs:488 守卫）——120s 头超时不会造成重复请求；failover 对 Connect/Timeout/Request 一律降级（02 票结论，此处冒泡路径一致）。

- **IPv6 断言实证**：Cargo.lock 锁定 http 1.4.2；`uri/authority.rs` `fn host` 对 `[` 开头 host 切片 `&host_port[0..i+1]`（含两个方括号）——`Uri::host()` 对 IPv6 字面量返回 `"[::1]"` 形态。

## P3 实施批（2026-09-10）

- **04-01 已修复**：`call()` 的「是否可重试」由 `timing.total_ms() == 0` 隐式判定改为显式 `was_reused` 布尔（新建连接的首发失败不再被误判可重试）；补注释说明重试的双重计费窗口（上游已受理后断连无法与发送前断连区分）。
- **04-02 已修复**：`parse_url` 显式拒绝 IPv6 字面量（`Uri::host()` 的方括号形态），报「暂不支持 IPv6 字面量上游地址」而非误导性的 DNS/TLS 失败；单测断言错误信息不含 DNS。
- **04-03 已修复**：`probe.rs` duration_ms 注释改写为「含建连与首字节等待（即 TTFT）、不含网关前置」；`metrics.rs` 与 `entity/request.rs` 的 tps 非流式分母括注改为 `end_time − ttft_start_ms`（新连接=建连开始、复用=请求发出）。
- **04-04 已修复**：删 `UpstreamCall.stream`（全仓无读者）与 `UpstreamReply.connect_done_at_ms`（只写不读），`build_native_upstream_call` 同步去掉 `client_stream` 参数；`StreamMetrics.connect_done_at` 改名 `ttft_start_at`（语义即 TTFT 起点）。
- **04-05 已修复**：新增 `probe::probe_preamble`（取最小模型 + 解密 key，失败返回阶段化 `ProbePreambleFailure`），`probe_provider` 与 `failure_recovery` 共用；日志仍留在各自调用层（恢复探测逐阶段点名供应商，测试断言文案不变）。路由段保持现状（404 语义需前置查 provider/model）。
- **04-06 已修复**：`test_model` 成功行复用调用期 `request_id`（原二次 uuid 生成导致同一次测试的日志与落库行断链）。
- **04-07 已修复**：新增 `retries_once_on_stale_pooled_connection` 集成测试（上游回响应后静默 close，第二次 call 断言重连成功 + 连接计数=2）。
- **04-08 已修复**：新增 `upstream::Timeouts` 可注入超时组（生产默认即原四常量）；`UpstreamPool::with_timeouts` 注入，`read_body` 从 `PooledBody::body_timeout()` 取。测试 `silent_upstream_times_out_with_injected_timeout`（silent upstream + 120ms 头超时 → Timeout 且不重试）。
- **04-09 已修复**：`parse_url` 单测 5 例（默认端口/空 path 与 query/缺 host/错误分支/IPv6 拒绝）；`connect_failures_map_to_connect_error`（连接拒绝与非 IPv6 无关的 URL 错误映射）；`proxy_defensive_branches_are_rejected`（非 http:// 前缀、userinfo 地址、代理 407 三条防御分支）。
- **04-10 已修复**：新增 `concurrent_calls_open_independent_connections_then_reuse`（同 key 4 并发各建一条连接、归还后第 5 次复用不新建）。
- **04-11 已拍板保持现状**：探活前先读用量缓存可省厂商调用，但当前每小时每家的调用量与恢复延迟在可接受范围；不改。
- **04-12 已拍板保持现状**：探活与真实流量同口径超时（边界探活失败=停用语义要求不被短超时误杀），UI 按钮已有 loading 防抖。
- **04-13 已登记观察**：每 host 并发连接无上限，当前并发画像（管理面 + 少量客户端）下无实际影响；性能放大时回看。

## 性能/内存轮结论

无 P1/P2。正向确认：空闲连接零拷贝复用（sender 直取）；每次新连接仅一次 TCP+TLS（rustls 异步握手，无阻塞池往返）；池表按 key 分离、Mutex 临界区为常数级 pop/push；每请求分配仅 key String 与 Vec 头——量级可忽略。P3 级观察见 04-13（每 host 并发无上限 + 突发滞留 600s）及其体内并入的 DNS 无超时/多地址串行微观察。结论：传输层性能形态适合当前并发画像（连接池命中率高、TTFT 计量精确），无需结构性改动。
