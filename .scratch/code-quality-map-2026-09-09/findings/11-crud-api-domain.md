# FINDINGS · 11 CRUD API 域审查（2026-09-10）

范围：`routes/` 管理 API 域 12 文件全读（providers 973 / virtual_models 917 / provider_models 882 / cron_jobs 379 / settings 290 / request_logs 285 / api_keys 204 / openai_compat 147 / backup 139 / provider_templates 75 / mod 56 / chat 34）+ 交叉核对（availability、cron/scheduler、app_settings、usage/persist、proxy/headers、auth/mod、static_assets、src/backup 实现体、db.rs 索引与 DDL）+ tests/ 相关集成测试盘点。方法：三个子代理分文件族逐行全读 + 所有保留发现的主代理行号磁盘复核。清单模式：不改代码。F9（request 表直查原始 SQL 家族）主审在本票，结论见专节。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 11-01 | P2【已修复 2026-09-10】 | 逻辑/时序 | 时区变更时 `reload_all_jobs` 在 `settings.update` 之前调用，调度器用旧时区重建——代码与自身注释矛盾；当前全部种子任务时区不敏感故零可见影响，一旦出现 tz 敏感 cron 行即升 P1 |
| 11-02 | P2【已修复 2026-09-10】 | 竞态 | 更新/删除供应商失效用量缓存与「在途真实抓取」无版本护栏：在途抓取可在失效后回写旧凭据用量，脏缓存最长存活一个 TTL（10 分钟） |
| 11-03 | P2【已修复 2026-09-10】 | 逻辑/健壮 | request_logs 分页 offset 用 u32 相乘：超大 page 溢出（dev panic→CatchPanic 500 / release 静默回绕返回错页） |
| 11-04 | P3 | 事务完整性 | `update_provider` 先提交字段更新再调可用性动作，动作失败返回 500 但字段已落库（部分成功却报失败） |
| 11-05 | P3 | 逻辑/i18n | `update_provider` 两处错误消息硬编码中文绕过 lang（英文环境收中文） |
| 11-06 | P3 | 逻辑/一致性 | cron `update_job` 先落库后改内存，内存更新失败不回滚 DB——DB 新值而列表展示旧值，重启才收敛 |
| 11-07 | P3 | 逻辑/边界 | 未加载进调度器的任务（无 handler）无法软删除：`find_by_name` 见行 → `soft_delete_job` 内存 miss → 404，行永远删不掉 |
| 11-08 | P3 | 逻辑/一致性 | Bool 设置值校验不 trim（`" true"` 判 400），与 Int/Float/Json/language/timezone 的 trim 口径不一致 |
| 11-09 | P3 | 模块间/缓存 | `delete_setting` 只保护 language/timezone：删 max_consecutive_failures / allow_list 不刷新 AppSettings 缓存，运行期与库分叉至重启 |
| 11-10 | P3 | 逻辑/错误码 | provider_models 刷新/测速的上游 502 用 `SCHEDULER_ERROR` 码，与 usage 路由的 `UPSTREAM_ERROR`（bad_gateway）不一致 |
| 11-11 | P3 | 逻辑/一致性 | 嵌套 `GET /providers/{id}/models` 对不存在供应商返回 200 空数组（注释自称有意），与同组 create/batch/refresh 的 404 不一致 |
| 11-12 | P3 | 逻辑/校验 | `custom_header` 只校验「合法 JSON」，非对象（数组/数字/字符串）可落库，转发层静默忽略——保存成功但完全不生效 |
| 11-13 | P3 | 逻辑/校验 | 备份导入不校验 provider `apiKey` 非空，绕过创建接口必填校验（空串加密落库） |
| 11-14 | P3 | 逻辑/校验 | 备份导入直写 `enable`/`disabled_reason`，注释自称 ADR-0003 镜像不变式但无任何一致性校验（篡改文件可达 enable=true+reason=manual 的不可能态） |
| 11-15 | P3 | 逻辑/健壮 | request_logs `row_to_entry` 用 filter_map 静默丢行，items 与 total 可不一致且无日志 |
| 11-16 | P3 | 逻辑/输入 | CSV 过滤参数非法分段静默忽略（`vmId=abc` 等同不过滤返回全量），无 400 |
| 11-17 | P3 | 逻辑/边界 | 公开路径精确串比较：`/api/auth/status/`（尾斜杠）误判需登录返回 401 |
| 11-18 | P3 | 逻辑/API 语义 | 未知 `/api/*`、`/v1/*` 路径经 SPA fallback 返回 HTML 200 而非 JSON 404（鉴权后的语义毛刺，非安全洞） |
| 11-19 | P3 | 简洁 | `providers.rs:443` `_api_key` 死变量（clone 后从未使用） |
| 11-20 | P3 | 简洁 | providers.rs 七处 not_found 中英样板未收 helper（provider_models.rs 已抽 `not_found_provider/model` 可对齐） |
| 11-21 | P3 | 模块间/缓存 | 备份导入整表替换供应商后未清 `provider_usage_cache`/`usage_mem`——AUTOINCREMENT 保证旧 id 不复用不会误读，属孤儿行累积 |
| 11-22 | P3 | 模块间 | `/v1` 头策略拆两半：allowlist 取数 + `select_forwardable_headers` 在路由层（openai_compat.rs:39-41），鉴权头/剥离清单/模板默认头在 proxy/headers.rs |
| 11-23 | P3 | 性能 | 三个列表端点无分页无上限：list_providers 全表+逐行双解密、provider_models 两个列表全量 |
| 11-24 | P3 | 性能 | `load_usage_map` 每供应商一次 DB 查询 N+1——persist 层已有 `read_usage_cache_many` 批读（全仓仅 lb.rs:426 一个消费方）未被路由层使用 |
| 11-25 | P3 | 性能/竞态 | 用量读端点（`/usage`、`/usage/estimate`）抓取无单飞，并发 GET 重复打上游——呼应 07-01（四路抓取收敛 mem.fetch_shared 单飞是同一默认解） |
| 11-26 | P3 | 性能 | request_logs：`modelId` 单独过滤用不上复合索引（前导列 provider_id）；按 `requestTime`/`totalTokens` 排序无索引全扫+排序 |
| 11-27 | P3 | 观察项 | `ProviderResponse` 不含 `disabled_reason`，管理端无法区分 failure/quota/manual 三类停用 |

**拍板记录（两项，均不改代码，结论锁定现状）**：
- **拍板一（原 B-06）**：创建/更新虚拟模型**允许**把已停用供应商的模型加为成员，保持现状——成员关系 ≠ 可用性，选路时 `traffic_available` 剔除、恢复后自动回归候选，语义归 availability 单点，创建侧不加校验。
- **拍板二（原 C-04）**：全局 `DefaultBodyLimit(5MB)`（mod.rs:43）保持现状作为 DoS 兜底，接受巨型多模态 /v1 请求被 413；若未来有真实大 payload 需求再单独放宽 /v1。

## 各条证据

### 11-01 时区变更重载用旧时区（P2，逻辑/时序）【已修复 2026-09-10】

`routes/settings.rs:190-213`：`reload_all_jobs`（:193）在 `active.update`（:209）与 `state.settings.update(&key, &req.value)`（:213）**之前**执行；而 `reload_all_jobs` 重建 job 与重算 next_run_at 读的时区来自进程内缓存 `self.settings.timezone()`（cron/scheduler.rs:321、:582；AppSettings::timezone 读 `inner.read()`，app_settings.rs:207-209），此刻缓存仍是旧值。代码注释（settings.rs:188-189）自称「先在内存里用新时区重建并重算 next_run_at，再落库」——实现与注释矛盾。对照组：备份导入路径顺序正确（routes/backup.rs:73-84 先逐行 `settings.update` 再 `reload_all_jobs`）。

**当前零可见影响**：全部内置任务 `@every 5m`/`@every 1h`/`@hourly`（cron/seed.rs:88-98）均时区不敏感（间隔制或每小时整点，任何时区同一刻），且无创建任务 API，tz 敏感 cron 行进不了库。一旦出现 `0 0 8 * * *` 类定点任务即升 P1（静默按旧时区触发，重启才自愈）。默认解：把 `reload_all_jobs` 移到 `settings.update` 之后（对齐 backup.rs 顺序），一行顺序调整；测试缺口正对【测试覆盖盘点】T1。

### 11-02 用量缓存失效与在途抓取竞态（P2，竞态）【已修复 2026-09-10】

失效点成对无误（providers.rs:534-537 更新后、:685-688 删除后，`invalidate_usage_cache` + `usage_mem.invalidate`）。写点无护栏：`fetch_and_store`（usage/persist.rs:127-137）先抓后写，`write_usage_cache`（:83-108）按 provider_id 无条件 upsert，无代次/`fetched_at` 比较。交错序列：GET /usage 或 LB 抓取在途（读的是旧凭据）→ PUT 更新提交并失效缓存 → 在途抓取返回回写旧数据 → 展示/选路/边界探活用旧账号用量直至下个 TTL 或 cron 刷新。窗口窄、自愈（≤10 分钟）、无数据损坏，故 P2。默认解（实施批）：写前比对 provider.updated_at 快照不一致则丢弃，或把失效与在途取消挂钩。

### 11-03 request_logs 分页 offset 溢出（P2，逻辑/健壮）【已修复 2026-09-10】

`request_logs.rs:131-136`：`page`/`page_size` 均 u32（:43-44），`let offset = ((page - 1) * page_size) as i64;`。page_size 上限 100，page > ~42,949,672 即 u32 乘法溢出。Cargo.toml 无 profile 覆盖：dev/test 默认 overflow-checks → panic（CatchPanic 转 500）；release 静默回绕 → offset 错、返回错误页数据（非报错）。默认解：先转 i64 再乘（一行）。测试缺口 T11 正对。

### 11-04 update_provider 部分成功报失败（P3，事务完整性）

providers.rs:492-505：`provider_repo::update_provider` 单独提交后，`enable_changed` 分支才调 `availability::enable_manual/disable_manual`；动作失败 `return response::db_error`（500），名称/BaseURL/协议等字段已持久化——前端收到失败但部分生效。缓解面：enable/disabled_reason 不在该 update 内写（From<Model> 全 Unchanged，只写 Set 列），不会留下半改启停态；协议级联（509-532）与缓存失效（534-537）失败仅 warn 为有意。默认解：字段更新+可用性动作收进同一事务，或失败时回读返回实际状态。

### 11-05 update_provider 硬编码中文（P3，i18n）

providers.rs:542 `Provider {id} 不存在`、:549-551 `同名 Provider 已存在，名称需要唯一` 均未按 lang 分支；同函数其余分支与 create_provider（399-403）都走了 lang，属遗漏。默认解：补 lang.tr 分支（i18n_integration 测试顺带补英文断言）。

### 11-06 cron 更新 DB/内存非原子（P3，一致性）

cron_jobs.rs:178-202：先 `repo.update_job_full` 落库，再 `scheduler.update_job_in_memory`；后者失败返回 scheduler_error 但 DB 已提交。`list_jobs_detailed` 的 title/expression/enabled 取自内存（scheduler.rs:505-522），于是接口展示旧值、库是新值，重启后反转。概率低（内存更新失败=调度器 remove/add 异常），P3。默认解：内存失败时用旧 model 回写 DB。

### 11-07 未加载任务无法软删除（P3，边界）

cron_jobs.rs:219-231：`repo.find_by_name` 只过滤 is_deleted（未加载任务的行可见）→ `soft_delete_job` 第一句 `jobs.get(name).ok_or(JobNotFound)`（scheduler.rs:481-487）→ 路由映射 404。后果：handler 未注册被跳过加载的任务在 API 里不可见（列表只列内存）、不可更新（400，约定内）、**不可删除**（404，行残留）。默认解：`soft_delete_job` 对内存 miss 退化为纯 DB 软删（repo.soft_delete 直接落）。

### 11-08 Bool 校验不 trim（P3，一致性）

settings.rs:81 `!matches!(value, "true" | "false")` 不 trim；Int（:68）、Float（:75）用 `value.trim().parse`、Json（:87）trim、language/timezone（:96/:104）trim。`" true"` 判 400 而同类数值/JSON 放行前后空白。默认解：Bool 分支加 trim。

### 11-09 删除非保护设置键不刷新缓存（P3，缓存）

settings.rs:263-273 只拒绝删 `language`/`timezone`（注释自述理由）；`max_consecutive_failures` 与 `downstream_request_header_allow_list` 可删，删后 AppSettings 缓存不更新（刷新只在 PUT :213 与启动），运行期用旧阈值/旧 allowlist 而 `/api/settings` 已无该行，重启种子回种默认值。默认解：删除成功后 `state.settings` 同步失效该键（或一并列入保护清单）。

### 11-10 上游 502 错误码不一致（P3，错误码）

provider_models.rs:691（刷新）、:846（测速）用 `scheduler_error(BAD_GATEWAY, msg)`（SCHEDULER_ERROR 码）；usage 路由（providers.rs:811、881）同类上游失败用 `bad_gateway`（UPSTREAM_ERROR）。前端按 code 分支时两类上游错误形状不同。默认解：统一 UPSTREAM_ERROR。

### 11-11 嵌套模型列表缺 404（P3，一致性）

provider_models.rs:228-233：供应商不存在按空列表返回（注释自称「与既有语义一致：级联删除后列表为空而非 404」，属有意）；同组 create（:364-368）/batch（:429-433）/refresh（:651-658）均 404。已删供应商的前端会得到「存在但空」假象。默认解（实施批评估）：补存在性检查返 404，或注释升级为文档化约定。

### 11-12 custom_header 非对象可落库（P3，校验）

providers.rs:182-188 经 `validate_json_field`（:297-307）只校验 `serde_json::from_str` 成功；`"32122"` 等非对象可落库；转发层 proxy/headers.rs:126-129 `let Some(map) = value.as_object() else { return }` 静默跳过——保存成功但请求头完全不生效，无任何提示。默认解：校验升级为「必须是 JSON 对象且值为字符串」（与转发层消费形状对齐）。

### 11-13 备份导入绕过 apiKey 必填（P3，校验）

创建路径 providers.rs:349-359 校验 api_key 非空；导入值校验 routes/backup.rs:101-126（protocol/billing/proxy/custom_header/extra + 成员 proxy）无 api_key 检查；src/backup.rs:464 直接 `crypto::encrypt(&p.api_key)` 落库。仅手工构造/篡改备份可达，危害面窄（该供应商上游必然鉴权失败）。默认解：`validate_import_values` 补非空校验。

### 11-14 备份导入不校验可用性镜像不变式（P3，校验）

src/backup.rs:458-475 直接 `Set(p.enable)` + `Set(p.disabled_reason)`，注释自称「停用原因镜像不变式（ADR-0003）：enable ⇔ disabled_reason」但 validate_backup（:299-343）与 validate_import_values 均未校验；读侧谓词 availability.rs:54-55 为 `enable && disabled_reason.is_none()`。篡改备份可造 `enable=true, reason=manual`（显示启用、实际不可选路）。正常写路径全走 availability 状态机，仅篡改文件可达，P3。默认解：导入校验 `p.enable == p.disabled_reason.is_none()`。

### 11-15 request_logs 行转换静默丢行（P3，健壮）

request_logs.rs:246-249 `rows.iter().filter_map(|row| row_to_entry(row).ok())`：任一字段 try_get 失败即丢行，`items.len()` 与 total 不一致且无日志。表结构稳定时不触发，属稳健性缺口。默认解：上抛 db_error 或至少 warn。

### 11-16 CSV 过滤参数静默失效（P3，输入）

request_logs.rs:61-66 `parse_csv_i32` 对分段 `filter_map(parse().ok())`：`vmId=abc` 得空 vec，`push_in_clause`（:83-85）空即 return → 等同不过滤返回全量而非 400。客户端拿到「看似成功」的错误结果。默认解：非法分段返 400。

### 11-17 公开路径尾斜杠边界（P3，边界）

auth/mod.rs:263-266 精确串比较（`path == "/api/auth/status"` 等）：`/api/auth/status/` 落入「/api/ 需会话」分支返 401。影响极窄（客户端一般不带尾斜杠）。默认解：比较前 `trim_end_matches('/')`。

### 11-18 未知 API 路径返回 HTML 200（P3，API 语义）

mod.rs:42 fallback 无条件挂 `serve_asset`；static_assets/mod.rs:72-78 未命中返回 index.html 200。已鉴权的 `/api/typo`、带有效 Bearer 的 `/v1/typo` 拿到 text/html 200 而非 JSON 404。未鉴权仍先 401，非安全洞。默认解：fallback 内对 `/api/`、`/v1/` 前缀返 JSON 404。

### 11-19 `_api_key` 死变量（P3，简洁）

providers.rs:439-443：`new_api_key` 已用于 :479 加密写回；`let _api_key = new_api_key.clone().unwrap_or_else(|| model.api_key.clone());` 之后全函数未再引用，一次无谓 clone。默认解：删行。

### 11-20 not_found 样板×7（P3，简洁）

providers.rs 七处同构中英 not_found（:419-423、:542、:635-639、:705-710、:735-739、:775-779、:852-858）；provider_models.rs:860-874 已抽 `not_found_provider`/`not_found_model` 可对齐。默认解：providers.rs 同款抽 helper。

### 11-21 备份导入不清用量缓存（P3，缓存）

src/backup.rs:424-450 `apply_import` 事务内清 virtual_model_item/virtual_model/provider_model/provider/api_key，但未删 `provider_usage_cache`、未失效 `usage_mem`（对比删除供应商路径 providers.rs:685-688 的成对失效）。provider.id 为 AUTOINCREMENT（db/app.db schema 实测 + sqlite_sequence 在册），旧 id 不复用 → 不会误读，属孤儿行累积。默认解：导入成功后清表+全量 invalidate（一行量级）。

### 11-22 /v1 头策略跨层（P3，模块间）

openai_compat.rs:39-41 路由层取 `settings.downstream_header_allow_list()` 并调 `proxy::select_forwardable_headers` 后传入 `forward_chat`；鉴权头优先级/剥离清单/模板默认头全在 proxy/headers.rs。功能干净无重复，仅「头策略」概念跨两层。默认解（实施批评估）：allowlist 读取下沉 forward_chat，路由层零头逻辑。

### 11-23 列表端点无界（P3，性能）

providers.rs:326-342 list_providers 全表 + 逐行 api_key/extra 双解密；provider_models.rs:224-252（按供应商全量）、:274-303（全局全量）。均无 limit/offset。供应商/模型为管理员配置量级（数十行级），当前可接受；规模增长后是线性放大点。默认解：图后如需再加分页；现记观察。

### 11-24 load_usage_map N+1（P3，性能）

virtual_models.rs:464-476：`for id in provider_ids { read_usage_cache(db, *id) }` 每供应商一次 DB 往返，列表端点（:532）对所有虚拟模型涉及的去重供应商逐个查。persist 层已有批读 `read_usage_cache_many`（persist.rs:45-65）全仓仅 lb.rs:426 消费且零测试（07-04 已记）。默认解：改用批读，一行替换 + 顺带消 07-04 的「零消费方」死角。

### 11-25 用量读端点抓取无单飞（P3，性能/竞态）【已修复 2026-09-10】

providers.rs:808-812（/usage）、:876-883（/usage/estimate）缓存未命中即直调 `fetch_and_store`；`mem_cache.fetch_shared_with`（mem_cache.rs:66-138）单飞只服务 LB（lb.rs:445）。并发 GET 同供应商重复真实抓取。默认解=07-01 同一方案：四路抓取收敛 mem.fetch_shared 单飞入口。

### 11-26 request_logs 索引慢路径（P3，性能）

db.rs 索引面：`idx_request_start_time`（:335 复合 start_time+provider_id+success）、`idx_request_provider_model_success_start`（:511 前导 provider_id）、`idx_request_ttft/tps`（:509-510）等。`modelId` 单独 IN 用不上 :511（前导列 provider_id）→ 全表扫；`ORDER BY request_time`/`total_tokens` 无任何索引 → 全扫+排序。page_size≤100 + 无无界导出兜底，当前表量级可接受。默认解：视查询频次补索引或收窄排序白名单（图后）。

### 11-27 ProviderResponse 缺 disabled_reason（P3，观察）

providers.rs:46-65 响应无 `disabled_reason`（实体有，entity/provider.rs 含该列）；前端 `Provider` 类型同样没有，disabledReason 仅出现在备份导出类型。管理端只见 enable=false，无法区分 failure/quota/manual，排障与恢复预期不透明。默认解：响应补字段（前端同步），或图后产品向再议。

## F9 主审结论（request 表直查原始 SQL 家族）

**安全面：全部无注入风险。** request_logs.rs（:139 起 `WHERE 1=1` + `push_in_clause` :77-89）列名全是调用方硬编码字面量、值一律 `sea_orm::Value` 走 `?` 绑定；排序列名经 `SORTABLE_COLUMNS` 白名单（:28-38、:193-197），方向 match 产常量；`:203/:220-228` 两处 `format!` 只插入上述常量片段与 `LIMIT ? OFFSET ?`。MODULES.md 记载「format! 拼 where_sql」形式属实但非注入面。providers.rs:927-937 现状已是静态 SQL + `Statement::from_sql_and_values` 全参数绑定（01 票登记的 format! 形态已不存在，**F9 在 providers.rs 的实例已自然消亡**）。stats 侧（summary_charts/rank_impl/metrics/insight）format! 只插 i64 时间戳与常量主体列表，用户过滤全走 `?`（10 票已顺带核验）。

**简洁面：属实。** 三套 WHERE 构造器并存互不共享（request_logs::push_in_clause / summary_charts::filter_parts / rank::push_rank_filters），cron 表侧已有 helper 化样板（cron_jobs.rs:283/298/333 → log_repository）可对照。属图后实施批的收敛项，非缺陷。

**唯一实质缺口**：11-03 分页溢出、11-15 静默丢行、11-16 非法过滤静默、11-26 慢路径（均单列）。

## 已核验无问题区（避免后续票重复审查）

- **级联删除事务**：delete_provider（providers.rs:645-694）单事务先删 virtual_model_item（按名下 model_ids）→ provider_model → provider，无悬空；delete_provider_model（provider_models.rs:606-643）先删成员再删模型，rows_affected==0 显式回滚 404；VM 删除同事务硬删成员（virtual_models.rs:872-899）。
- **可用性写路径单一 owner**：update_provider 不写 enable/disabled_reason（ActiveModel 全 Unchanged 只写 Set 列），启停全走 availability 动作；探活/门控不误触 manual 态。ADR-0003 未被路由层破坏。
- **密钥面**：列表/详情 api_key_masked（providers.rs:69/311-316），明文仅 `GET /{id}/api-key`（:720-743 会话保护）；创建/更新只加密提交值，全库无「解密存储值再写回」路径（decrypt_or_passthrough 只读），extra 清空教训的防线守住。api_keys：`lg-`+16 字节随机、明文只回一次、key_hash SHA-256 索引查找。
- **缓存失效成对**：更新/删除供应商均 DB+mem 双失效；创建无缓存需失效。
- **协议级联**：供应商/模型协议变更调 `remove_mismatched_members`（providers.rs:509-532、provider_models.rs:575-588、virtual_models.rs:828）；`effective_protocols` 尊重模型级覆盖再回落供应商。
- **用量预估信任边界（645bca1 后守住）**：providers.rs:863-955 限 billing_mode==1+usage_enabled，SQL 仅 provider_id+success=1+半开窗口求和（COALESCE 容 NULL），used_tokens==0 或不可折算一律 estimatable=false；resets_at 缺失退化空窗口不产错值；5 状态集成测试在册。
- **cron 约定**：未加载任务更新 400 不碰库（cron_jobs.rs:134-141 先于任何 DB 写，测试锁定）；禁用=调度器移除委托 scheduler 正确表达；日志端点 run 归属校验防越权读（:300-313）；SSE 先订阅后读快照无丢事件窗口（:335-339）；cron 表侧 SQL 全经 log_repository（F9 正面样板）。
- **settings 校验面**：Int/Float/Bool/Json 类型 + language/timezone/threshold 值级 + allowlist 三层（JSON 数组/合法头名/剥离清单拒写）齐备；未知 key 404；非时区键写后缓存热生效正确（:213），language 追加任务文案同步（:216-239）。
- **路由守卫**：healthz 与 auth status/login/init 公开（auth/mod.rs:263-266）；其余 /api/* 会话（:293-318）；/v1/* Bearer 在 auth/mod.rs:274-291 挂载（mod.rs 无）、/v1/messages 额外接受 x-api-key；fallback 在守卫后、SPA 不拦截约定成立；logout/me/change-password 均需会话。
- **openai_compat / chat / provider_templates 薄而干净**：/v1/models 与详情一致过滤 Enable+CHAT_SERVED_TYPES；chat 仅空消息 400 后委托 forward_chat_direct；templates 纯 DB 匹配无网络调用无 SSRF。
- **批量创建去重**：provider_models.rs:415-515 批内尾段去重 + 库内尾段忽略大小写比对，事务内插入唯一冲突回滚。
- **重排原子性**：reorder_providers（565-626）校验在前、事务内全量校验后逐个写、失败靠 txn drop 回滚。
- **N+1 基本规避**：provider_names_by_id 批量取名（provider_models.rs:207-222）、全局列表一次 IN 查询——唯一例外 11-24。
- **备份校验骨架**：版本精确匹配、顶层字段、枚举范围（与 virtual_models.rs 口径一致）、自然键唯一与引用可解析、设置类型名、单事务整体替换回滚、导入后设置缓存同步+时区重建**顺序正确**（11-01 的对照组）。
- **导出含明文密钥**属设计（会话保护+文档声明），不重复判定。

## 测试覆盖盘点

已有：providers 26 用例（加密/脱敏/重名/reorder/代理/启停清 failure 等）；provider_models 四子模块（CRUD/校验矩阵/批量去重/级联/协议覆盖/代理刷新）+ test_model 8 例；usage_estimate 5 状态；provider_usage/cache 6 例（含更新后失效）；virtual_models 三子模块（CRUD/级联/接口矩阵/排序）；cron_jobs（未加载 400 不碰库/软删/立即执行等）；cron_job_logs（runs/logs 形状/404）；settings（类型矩阵/allowlist/内置键保护）；api_keys 9；request_logs 6；backup 8；chat 6；i18n 仅 providers/settings 英文。

**缺口清单**（按可实施性排序）：
- T1 时区变更触发 reload_all_jobs 的顺序回归（正对 11-01，断言重建读到的时区为新值）。
- T2 SSE `/cron-jobs/{name}/logs/stream` 后端零集成测试（snapshot/idle/log/reset/Lagged 全裸，仅前端 MockEventSource 覆盖）。
- T3 竞态路径零测试：更新-删除供应商、启停-门控交错、缓存失效-在途抓取（11-02）、cron 内存失败回滚（11-06）。
- T4 供应商级 protocol_type/billing_mode 越界、proxy_addr 含 `@`（模型级有供应商级没有）。
- T5 删供应商级联 virtual_model_item 无直测（级联 provider_model 有）；更新供应商协议触发 remove_mismatched_members 无测；删供应商后用量缓存失效无测（更新有）。
- T6 刷新 smart/partial 匹配态无测（仅 pending/manual）。
- T7 custom_header 非对象（11-12）、ProviderApiKey 解密失败空串、批量创建非法 protocol_type、更新模型 provider_id 不匹配。
- T8 cron：已加载任务非法表达式 400、enabled=false 后调度器实际移除、未加载任务删除（11-07）。
- T9 settings：language 变更同步任务文案、max_consecutive_failures<1、删除非保护键后缓存行为（11-09）、Bool 带空白（11-08）。
- T10 api_keys：401 未认证、超长 name、缺 enable 字段 422、解密失败掩码空串、列表排序断言。
- T11 request_logs：大 page 溢出（11-03）、page=0、pageSize clamp 边界、end_time 边界毫秒开区间、非法 CSV 过滤（11-16）、行转换失败（11-15）、401。
- T12 backup：重名 provider/displayId/apiKey、枚举越界 400、apiKey 空（11-13）、enable/reason 不一致（11-14）、>5MB 413、空 providers 清库、缺顶层字段、未知设置类型名。
- T13 i18n：api_keys/request_logs/backup/chat/provider_templates 英文分支全裸。
- T14 provider_templates 无专属测试文件（仅 providers_integration 间接覆盖命中/多命中/未命中/空 URL）。

## 性能/内存轮结论

无 P1/P2 性能项。正向：列表域无经典 N+1（除 11-24）；request_logs count+list 固定双查询、page_size≤100、无无界导出；SSE 每连接一个 broadcast 订阅+15s KeepAlive 断开即 drop（容量 8192、Lagged 转 reset）；cron 列表内存 map+一次 list_by_names；settings/backup/api_keys 均为配置量级。P3 级集中四点：11-23（无分页列表+逐行解密）、11-24（usage map N+1，有现成批读可换）、11-25（用量抓取无单飞，默认解=07-01 收敛单飞）、11-26（modelId 过滤与 requestTime/totalTokens 排序无索引）。备份导入整 JSON 读入内存+设置逐条 upsert（N+1）为配置量级可接受。结论：形态适合当前规模，唯一值得实施批优先的是 11-24（一行替换顺带消死角）与 11-25（与 07-01 合并实施）。

## 实施进度

- **11-01 已修复**：`settings.rs::update_setting` 的 `reload_all_jobs` 从「落库前」移到「落库 + 缓存刷新后」——重算 next_run_at 读进程内时区缓存，顺序反了会按旧时区重建（对齐 backup 导入路径的正确顺序）。
- **11-02 已修复**：`UsageMemCache` 增加 provider 失效代次（`invalidate` 自增、`generation()` 读取），单飞抓取在开始前记录代次、写库与回填内存前比对，期间发生失效即作废结果（新增 `UsageError::Stale`，502 类可重试）；单测 `mem_cache_fetch_discards_result_when_invalidated_midflight`（先红后绿已验）。ADR-0009 Decision 5。
- **11-03 已修复**：`request_logs.rs` offset 改 `(i64::from(page) - 1) * i64::from(page_size)`，u32 乘法溢出（dev panic / release 静默回绕）消除。
- **11-25 已修复**：`/usage` 与 `/usage/estimate` 两个读端点改用 `usage_mem.fetch_shared_stored`（含新鲜度短路 + 单飞 + 代次护栏），与 LB 兜底共用同一入口，并发 GET 不再重复打上游。
