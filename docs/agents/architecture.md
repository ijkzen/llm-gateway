# 架构与模块地图

`llm-gateway` 的整体形状、目录结构与模块边界。各域的语义细节见同目录其它文档（见文末索引）。

## 运行方式

后端启动后监听 `0.0.0.0:4007`，前端 SPA 以静态资源形式内嵌在后端中，通过 `/api/*` 与后端通信；对外转发面为 `/v1/*`。

## 基础能力

- **认证（单用户）**：首次启动走初始化流程创建管理员（argon2 哈希），Cookie Session（7 天）保护 `/api/*`；设置页可修改密码（吊销其他会话）。`src/auth/` 同时提供 `/api/*` 会话中间件与 `/v1/*` Bearer 中间件。
- **系统设置**：列出、按声明类型校验并修改键值（`String`/`Float`/`Int`/`Bool`/`Json`；`Json` 类型在设置页用结构化表单弹窗逐行增删键值，嵌套 JSON 退回原文编辑）。`AppSettings` 在内存缓存并热生效。
- **供应商 API Key 管理**：服务端生成 `lg-` 密钥，AES-256-GCM 加密存储（含 SHA-256 `key_hash` 供鉴权查找）。
- **备份恢复**：全量导出/导入 JSON（ADR-0017）。
- **静态资源**：`rust-embed` 将 `web/dist` 打包进二进制。

## 项目结构

```text
.
├── Cargo.toml              # Rust 项目配置
├── Cargo.lock              # Rust 依赖锁定
├── Dockerfile              # 多阶段构建镜像
├── .env.example            # 环境变量示例
├── src/                    # 后端源码
│   ├── main.rs             # 入口：加载配置并启动服务
│   ├── lib.rs              # 模块导出与 run() 生命周期（含优雅关闭）
│   ├── config/mod.rs       # 环境变量与 Config 结构
│   ├── state.rs            # AppState（db + scheduler + lb_state + usage_cache + speed_test 槽位）
│   ├── response.rs         # 统一 API 响应结构（Response<T>/错误码/helper）
│   ├── app_settings.rs     # 语言/时区/熔断阈值等设置项的进程内缓存与种子（含热更新的 allowlist 等）
│   ├── availability.rs     # 供应商可用性状态机（ADR-0003）
│   ├── backup.rs           # 备份 JSON 构建/解析与校验（ADR-0017）
│   ├── crypto/mod.rs       # AES-256-GCM 敏感字段加解密
│   ├── i18n.rs             # API 消息本地化（调用点即翻译点：中文原文 + 英文译文，按全局语言选择）
│   ├── provider_repo.rs    # Provider 域 CRUD 写操作 DAO（插入/更新/删除 + extra 加密回填；启停状态由 availability 承载）
│   ├── failure_recovery.rs # 连续失败禁用供应商的自动恢复探测（每小时 handler 业务体）
│   ├── logs_cleanup.rs     # 日志过期清理
│   ├── db.rs               # SeaORM 连接、连接池与自动建表/迁移（单元测试在 db/tests.rs）
│   ├── db/tests.rs         # 迁移与连接配置的单元测试（主文件瘦身迁出）
│   ├── middleware/mod.rs   # CORS、Trace、CatchPanic 中间件
│   ├── static_assets/mod.rs# rust-embed 内嵌前端 dist
│   ├── auth/mod.rs         # 登录认证：argon2 密码哈希、session 表、/api 与 /v1 拦截中间件
│   ├── provider_model/     # 供应商模型：catalog（模型目录读取/搜索）、catalog_refresh（抓取校验原子写）、
│   │                       # refresh、data/（内嵌目录快照，ADR-0001/0025）
│   ├── provider_template/  # 供应商模板：mod（模板匹配 + template_default_headers）、
│   │                       # seed（按厂商预置模板，分 chunk 存放）、tests
│   ├── proxy/              # /v1 转发核心模块（按转发阶段拆分，单文件 ≤1000 行）
│   │   ├── mod.rs          # 门面：子模块声明 + pub use 重导出（forward_chat/LbState/test_model 等路径不变）
│   │   ├── lb.rs           # 协议枚举/LB 状态/成员加载与用量感知排序（Protocol/LbState/Member/resolve_proxy/order_members）
│   │   ├── headers.rs      # 出站头四层组装：下游透传/自定义/模板默认/协议鉴权 + 剥离清单
│   │   ├── calls.rs        # 上游调用组装（build_upstream_call / build_native_upstream_call）
│   │   ├── forward.rs      # 转发入口：forward_chat（/v1 chat）+ forward_chat_direct（管理后台直连）
│   │   ├── route.rs        # 路由解析前半段 resolve_and_order（display_id 路由/接口类型门/成员加载/LB 排序与选路日志统一形状）+ RouteError
│   │   ├── failover.rs     # 统一成员尝试循环 forward_through_members + ForwardFlavor/成员结局类型
│   │   ├── failure_recheck.rs # 失败复查（ADR-0012）
│   │   ├── native.rs       # /v1/messages・/v1/responses 原生透传（forward_native + 旁路 usage 扫描）
│   │   ├── dispatch.rs     # 成功分派/流收集/指标落库（dispatch_success/accumulate_chunks/record_failure）
│   │   ├── relay.rs        # 统一流式转运泵（PumpSource 事件源/TailSpec 收尾策略/StreamOutcome 记账，四协议臂共享；Converter 门面在此）
│   │   ├── probe.rs        # 测速与探活（test_model/probe_provider/probe_preamble/ProbeFailure）
│   │   ├── upstream.rs     # 上游 HTTP 客户端（hyper + tokio-rustls，连接池化复用，TTFT 起点=建连开始/请求发出）
│   │   ├── pool.rs         # 上游连接池（按 host 隔离，响应体读完归还，空闲 10 分钟释放）
│   │   ├── convert/        # 协议转换：openai(直通)/responses/anthropic/gemini 四协议，
│   │   │                   # 每协议按 request/response(+images/stream) 拆分，xxx.rs 重导出
│   │   ├── metrics.rs      # request 表记录与流式指标（ttft/输出耗时）
│   │   ├── sse.rs          # SSE 拆分/写出工具
│   │   └── usage_rank.rs   # 用量感知排序纯比较器（订阅 FEFO 截止时间链 / 按量余额）
│   ├── notification/       # 飞书通知渠道（ADR-0023）：mod.rs（配置读写/加密掩码/测试发送）、
│   │                       # feishu.rs（取 token + 发消息，手写 HTTP）、register.rs（扫码注册会话）、
│   │                       # notify.rs（自动转换 → 文案 → 异步发送）
│   ├── speedtest/          # 模型测速（ADR-0027）：长文流式请求测 TPS，结果落库只留最近一次运行
│   │   ├── mod.rs          # 门面：SpeedTestSlot（单实例 + 取消旗标 + SlotGuard）/内置题面与常量/target_max_tokens
│   │   ├── candidate.rs    # 候选集与过滤单一事实源（启用门槛 → 虚拟模型 → 关键词三字段），预览与启动共用
│   │   ├── bench.rs        # 单模型流式采样（build_upstream_call + PumpSource::feed_for_metrics + StreamMetrics）
│   │   ├── store.rs        # 运行/结果读写（单行语义 clear_previous、pending 占位行、启动改判 interrupted）
│   │   └── runner.rs       # 运行控制（单实例/可取消/无并发上限；无单模型硬超时）
│   ├── usage/              # 供应商用量查询：按 base_url host 分发 fetcher，归一化输出
│   │   ├── types.rs        # UsageData/QuotaWindow（available 标记三窗）/BalanceItem
│   │   ├── estimate.rs     # 用量预估（ADR-0008 纯核）
│   │   ├── error.rs        # 用量查询错误类型
│   │   ├── persist.rs      # 用量数据库缓存（10 分钟新鲜度，cache_age_fresh* 单一判定）+ 全量刷新 + 订阅额度耗尽自动停用/恢复 + 订阅制边界实测探活（probe_boundary_providers）
│   │   ├── mem_cache.rs    # LB 选路用量内存缓存（与 DB 缓存同新鲜度 + provider 单飞抓取去重）
│   │   ├── http.rs         # reqwest 封装（15s 超时；LLM_GATEWAY_USAGE_HTTP_OVERRIDE 供测试重定向）
│   │   ├── cookiecloud.rs  # CookieCloud 解密（MD5 材料 + EVP_BytesToKey + AES-256-CBC）
│   │   ├── sensenova_login.rs # 商汤登录态换取
│   │   ├── volcengine_sign.rs # 火山 V4 签名（service=ark，scope 以 /request 结尾）
│   │   └── fetchers/       # 各厂商：api_key/balance/cloud_balance(阿里BSS+火山billing AK/SK)/copilot/
│   │                       # volcengine/xiaomi/stepfun/alibaba/qianwenai/krill/sensenova/siliconflow/
│   │                       # agentrouter/tokenrhythm/opendesign
│   ├── cron/               # 定时任务核心模块（详见 cron.md）
│   │   ├── mod.rs          # JobContext/JobHandler/JobInfo/SchedulerError 定义
│   │   ├── parser.rs       # 表达式解析、下次运行时间与频率计算（服务器本地时区，部署即东八区）
│   │   ├── repository.rs   # CronJobRepository trait 与 SeaORM 实现
│   │   ├── scheduler.rs    # SchedulerRuntime：任务加载、启停、增删改
│   │   ├── scheduler/tests.rs # scheduler 单元测试（FailingRepo 用宏生成透传委托）
│   │   ├── seed.rs         # 内置定时任务种子行（启动幂等 upsert）
│   │   ├── worker.rs       # JobWorker：有界队列 + 信号量并发池 + 优雅关闭 + 执行日志落库
│   │   ├── log_capture.rs  # JobLogLayer：捕获任务 span 内 tracing 日志 → broadcast
│   │   ├── log_repository.rs # 执行日志持久化（runs/logs 表、30 次清理、启动恢复）
│   │   └── test_utils.rs   # #[cfg(test)] 单元测试共享辅助（setup_db/sample_job）
│   ├── stats_snapshot/     # 统计快照（ADR-0021，详见 stats.md）
│   │   ├── core.rs         # 四级桶帧/闭桶判定/窗口分解/自然历法周期助手
│   │   ├── registry.rs     # 指标与主体注册表：16 可加和指标单一事实源，读写两侧谓词同文本
│   │   ├── generator.rs    # 闭桶固化：GROUPING SETS 主体模式 + 分位标量 + 哨兵行
│   │   ├── reader.rs       # 读路径覆盖计划与兑底降级
│   │   ├── subject.rs      # 主体键解析/单主体降级守卫/api_key 归并
│   │   ├── math.rs         # 比率与分位运算helper
│   │   ├── tasks.rs        # 内置任务/全量回填/自愈/时区重算 + 开始/结束运行日志
│   │   └── jobs.rs         # 两个内置任务的 handler 工厂，错误交回 worker
│   ├── routes/             # Axum 路由
│   │   ├── mod.rs          # create_app(state)：路由组装 + 登录拦截中间件
│   │   ├── auth.rs         # status/init/login/logout/me/change-password
│   │   ├── cron_jobs.rs    # 任务 CRUD + logs 列表/单次日志/SSE 实时流
│   │   ├── openai_compat.rs# /v1/models 元数据 + /v1/chat/completions 转发入口
│   │   ├── v1_providers.rs # /v1 对外只读接口（Bearer）：GET /v1/providers（ID 列表，
│   │   │                   # 不暴露敏感配置）+ GET /v1/providers/{id}/usage（与
│   │   │                   # /api 用量接口共享 providers::provider_usage_response 核心）
│   │   ├── chat.rs         # 管理后台直连聊天入口（forward_chat_direct）
│   │   ├── providers.rs    # 供应商 CRUD + 排序 + 详情 + api_key 查看 + 用量/预估
│   │   ├── provider_models.rs # 供应商模型 CRUD/批量创建 + 目录搜索 + 测速（scoped/global 两组路由）
│   │   ├── provider_templates.rs # 模板匹配（按 base_url 预填默认头）
│   │   ├── virtual_models.rs # 虚拟模型 CRUD + 成员管理与用量聚合
│   │   ├── api_keys.rs     # API Key CRUD + 详情 + 明文查看
│   │   ├── request_logs.rs # 请求日志：服务端分页/过滤/排序 + 行详情
│   │   ├── backup.rs       # 备份导出/导入
│   │   ├── stats.rs        # 数据面板 12 路由注册（门面 + stats/ 子目录：window/compute/
│   │   │                   # summary_charts/insight/rank/rank_impl/rank_snap/metrics，均 ≤1000 行）
│   │   ├── notification.rs # 通知渠道配置 CRUD + 测试发送 + 扫码注册 begin/poll
│   │   ├── speed_test.rs   # 模型测速 4 路由（候选预览/读运行/启动/取消）
│   │   └── settings.rs     # 设置列表与更新
│   └── entity/             # SeaORM 实体
│       ├── mod.rs
│       ├── cron_job.rs / cron_job_run.rs（一次执行的元信息）/ cron_job_log.rs（单条日志）
│       ├── user.rs（单用户，argon2 哈希）/ session.rs（主键为令牌 SHA-256）
│       ├── provider.rs / provider_model.rs / provider_template.rs
│       ├── virtual_model.rs / virtual_model_item.rs（成员映射）
│       ├── api_key.rs      # 供应商 API Key（lg- 前缀 + key_hash）
│       ├── request.rs      # /v1 每次转发的指标记录（19 个字段）
│       ├── usage_cache.rs  # provider_usage_cache：provider_id 唯一 + usage_json + fetched_at
│       ├── notification_channel.rs # 通知渠道配置（单行表：channel 主键 + 加密 config + enable + last_error/last_sent_at）
│       ├── snapshot.rs     # request_log_snapshot EAV 行（duration_type/start_time/end_time/
│       │                   # entity_type/entity/metric_type/metric_value）
│       ├── snapshot_meta.rs# 快照生成元信息（键值：时区偏移/各粒度水位/predicate_version）
│       ├── speed_test_run.rs   # 测速运行元信息（四态 + 过滤/题面快照 + 候选总数）
│       ├── speed_test_result.rs # 测速成绩行（冗余供应商名等快照字段 + request_id 审计指针）
│       └── setting.rs
├── tests/                  # 集成测试（36 个顶层文件 + common/ + 六个按域拆分的子目录）
│   ├── common/mod.rs       # 集成测试共享引导（内存库 + worker + scheduler；build_authed_app 自动注入测试凭证）
│   ├── auth_integration.rs # 认证：init/登录/拦截/改密踢会话/登出/Bearer
│   ├── proxy_integration.rs# 转发：四协议转换、include_usage 注入、failover、落库、LB 用量排序
│   │                       # （超 1000 行按域拆：tests/proxy_integration/ 下 protocol/failover/
│   │                       # outbound_headers/native_messages/native_responses 子模块）
│   ├── stats_snapshot_integration.rs # 快照↔实时等价（统计改动后的第一道防线，见 stats.md）
│   ├── speed_test_integration.rs     # 测速全链路（见 build-and-test.md）
│   ├── provider_boundary_probe_integration.rs # 订阅制窗口剩余 (0,1)% 边界实测：失败停用/成功保持与恢复/禁用后继续探活不抖动/manual 不探活
│   ├── provider_quota_gate_integration.rs # 订阅额度耗尽自动停用/恢复 + usage_refresh 种子任务被调度
│   ├── provider_failure_recovery_integration.rs # 连续失败熔断后的自动恢复探测
│   ├── upstream_pool_integration.rs # 上游连接池：复用 / 空闲释放 / Connection: close 不归还
│   ├── lb_48_scenarios.rs / lb_failure_disable_integration.rs # LB 场景矩阵与失败停用
│   ├── *_race_integration.rs # provider/provider_model/virtual_model 三个 rank 端点的赛马视图
│   ├── models_refresh_integration.rs / stats_snapshot_jobs_integration.rs # 内置任务接线
│   └── settings / providers / virtual_models / request_logs / notification / backup / i18n /
│       v1_providers / provider_usage / usage_estimate / provider_schema_check / model_metrics 等
├── web/                    # 前端源码
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── tailwind.config.ts
│   ├── postcss.config.js
│   ├── components.json     # shadcn/ui 配置
│   ├── index.html
│   └── src/
│       ├── App.tsx
│       ├── main.tsx
│       ├── index.css
│       ├── lib/            # api.ts（ky 封装）、utils.ts、constants.ts
│       ├── hooks/          # use-cron-jobs、use-cron-job-logs（runs/单次日志/SSE 实时流）、use-settings、use-theme（light/dark/system）、use-toast（sonner 封装）、use-debounced-value（共享防抖值）、use-speed-test（候选预览/运行 2s 轮询/启动/取消）等
│       ├── components/     # UI 组件（ui/ 为 shadcn/ui 基础组件，data-table/ 为 react-table 封装，
│       │                   # cron-jobs/ 下含 CronJobLogsDialog 日志弹窗等，另有 confirm-dialog、
│       │                   # skip-to-main、theme-toggle 等共享组件；metric-race-card 为四张赛马卡
│       │                   # 的泛型骨架，settings/ 下含 use-setting-submit-callbacks 域内 helper，
│       │                   # speed-test/ 下为测速面板/过滤栏/汇总条/结果表）
│       └── pages/          # overview、cron-jobs、settings、speed-test、not-found（404）页面
├── db/                     # 开发用 SQLite 数据库目录
├── logs/                   # 开发用日志目录
└── .github/workflows/      # GitHub Actions CI（ci.yml 测试 / nightly.yml 推送 nightly / release.yml 发布）
```

## 索引

| 文档 | 内容 |
| --- | --- |
| `gateway.md` | `/v1` 转发：四协议转换、failover、出站头、思考块透传、请求指标、LB 用量感知排序 |
| `providers.md` | 供应商/虚拟模型、用量查询与缓存、额度门控、飞书通知、模型测速、模型目录 |
| `stats.md` | 数据面板、统计口径（排除探测流量）、统计快照 |
| `cron.md` | 定时任务子系统与执行日志 |
| `build-and-test.md` | 环境变量、构建运行、Docker、数据库迁移、测试 |
| `deploy.md` | CI/CD、发布、部署与安全 |
