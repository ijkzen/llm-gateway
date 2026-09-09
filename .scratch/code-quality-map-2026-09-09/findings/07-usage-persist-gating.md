# FINDINGS · 07 usage 持久化与额度门控审查（2026-09-10）

范围：`types.rs`（690 行全读：窗口语义/可用判定/访问器）、`persist.rs`（651 行全读：缓存读写/全量刷新/apply_usage_gate/probe_boundary_providers + 测试区）、`mem_cache.rs`（230 行全读：10 分钟内存缓存/单飞）、`availability.rs`（门控动作面 145-352 全读 + 测试盘点，ADR-0003 状态机）+ 消费方交叉核对（lb.rs 三层读路径、routes/providers.rs ?refresh=1 与详情、virtual_models rank、failure_recovery/failure_recheck、lib.rs usage_refresh handler、cron seed）。方法：四文件全读 + 谓词/新鲜度判定全仓 grep 查第二份拷贝 + 测试缺口盘点。清单模式：不改代码。**本票无 P1/P2、无需要拍板的行为分裂点**（全部默认解明确，直接落清单；probe 语义与 ADR-0010、恢复双通道设计已拍板过不重问）。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 07-01 | P3 | 逻辑/单飞与负缓存 | 真实抓取入口无跨调用单飞且失败无负缓存：cron 刷新 / LB 第三层 / ?refresh=1 / failure_recheck 四路可并发重复抓同一家；持续故障（尤其 DB 写失败）期 LB 每请求一次真实厂商调用——E7 修的是分类（不静默）非放大 |
| 07-02 | P3 | 简洁/缓存双写面 | cron 刷新只写 DB 缓存，mem 缓存持有旧数据直到自身过期——刷新后 mem 命中反而返回旧一轮数据（同一 fetched_at TTL 口径、数据新旧差一轮刷新）；刷新成功后可顺带回填/失效 mem |
| 07-03 | P3 | 健壮/探活时长 | probe_boundary_providers 顺序执行边界供应商、单家最坏 ~260s（04 票同款超时形态）无总体时限——多家边界时拖垮 5 分钟周期（try_lock 自愈跳轮）；建议并发上限或总时限 |
| 07-04 | P3 | 测试覆盖 | mem_cache 失败/取消路径零测试（waiter 收 None 不悬挂、创建者取消清理的 Cleanup guard 无锁定）；`read_usage_cache_many` 全仓零测试；fetch_and_store 的 Database 错误传播无单测 |
| 07-05 | P3 | 测试覆盖 | probe_boundary_providers 零单测（判定/停用/恢复/跳过分支仅靠集成 4 场景）；?refresh=1 与定时刷新并发路径无测试 |

## 各条证据

### 07-01 抓取入口无跨调用单飞 + 失败无负缓存（P3，逻辑/单飞）

`fetch_and_store`（persist.rs:127-137）的直接调用方有四路：usage_refresh 定时刷新（refresh_all_usage 内 JoinSet，persist.rs:163）、LB 选路第三层（lb.rs:445-447 经 mem.fetch_shared——**唯一有单飞**的一路）、路由 ?refresh=1（providers.rs:808 直调）、failure_recheck（failure_recheck.rs:72）。mem 的单飞只覆盖 LB 内部并发；cron 5 分钟一轮与手动刷新/转发复查同刻命中同一供应商时各自真实抓取（upsert 幂等无数据损坏，代价=厂商 API 双调用 + 双写）。

**失败无负缓存**：抓取失败时 DB 与 mem 均不写入（fetch_and_store 先 fetch 后 write，失败即无新数据；mem 只在成功时 store，mem_cache.rs:118-124）——过期缓存被读侧按缺失处理，下一次选路再次触发真实抓取。持续故障场景的放大形态：
- 上游厂商故障：每请求一次真实调用（并发经单飞去重），单请求被厂商 15s 超时拖住（usage/http.rs:13）；
- **DB 写失败**（E7 已把落库失败改为如实报错不静默，persist.rs:122-126 注释）：厂商调用已发生（已计费）而缓存写不进——每次选路 miss 都再付一次厂商调用，直到 DB 恢复。E7 修复解决了「静默吞错 + 缓存永远不新鲜」的盲区，但「DB 故障期无退避反复真实抓取」的放大面仍在（sqlite 单文件故障面小，517 类瞬时锁竞争曾实发生，见生产排障史）。

默认解（实施批评估）：mem 层加失败负缓存（如 last_attempt 时间戳，60s 内不再真实抓取直接按 None/缺失处理）或把四路抓取统一收敛到 mem.fetch_shared 单飞入口（route/failure_recheck 传入 db 即可复用）。倾向后者——入口单飞化顺带解决 07-01 前半。

### 07-02 cron 刷新后 mem 缓存持有旧数据（P3，缓存双写面）

两缓存同用 `fetched_at` + `cache_age_fresh*` 新鲜度判定（全仓唯一实现，persist.rs:68-79，grep 无第二份拷贝 ✓），但**写入面不对称**：cron 的 refresh_all_usage 只写 DB（fetch_and_store → write_usage_cache）；mem 只在 LB 路径 store（lb.rs:431 第二层 DB 直出回填、mem_cache.rs:120 第三层成功回填）。场景：mem 持 T-9min 数据（新鲜，未过期）→ cron 在 T-0 刷新 DB 得新数据 → 下一次选路 mem 命中返回 9 分钟旧数据（读侧不再查 DB，lb.rs:412-417 第一层即返回）——数据新旧差一轮刷新（≤10 分钟语义内自洽，但「刚刷新完反而用旧」）。影响：用量排序/门控候选口径最大滞后 10 分钟（与 DB TTL 语义一致），非正确性问题。默认解：refresh_all_usage 成功后对成功家 `usage_mem.invalidate`（或 store 回填）——一行联动；需 AppState 传入（refresh_all_usage 现只收 db，可改收 &AppState 或返回成功 id 清单由 handler 联动）。

### 07-03 probe_boundary_providers 无总体时长上限（P3，健壮）

persist.rs:238-310：逐家顺序探活（`for p in providers` 串行 await probe_provider），每家最坏 = 建连 20s + HEADER 120s + BODY 120s ≈ 260s（04 票已记 probe 超时形态）。多家同时处于 (0,1) 边界时整个 usage_refresh handler 可被拖过 5 分钟周期——handler 有 try_lock 自愈（lib.rs:171-173，超时跳轮），代价=下一轮刷新与门控顺延、边界实测恢复延迟。生产实证面窄（边界区本应少见），观察级。默认解：探活并发上限（Semaphore 2-4，同 refresh_all_usage 形态）或整轮总时限；倾向并发上限（改动小且顺带缩短最坏时长）。

### 07-04 缓存/单飞失败路径零单测（P3，测试覆盖）

- `mem_cache.rs` 测试仅 3 例且全为成功路径（新鲜/过期、invalidate、4 路并发单飞成功）；**失败与取消路径零测试**：创建者 fetch 返回 None 时 waiter 是否收到 None 不悬挂（Cleanup guard 的 drop 通知设计 mem_cache.rs:101-111 有注释无测试）、创建者 future 被取消（tokio 任务 abort）时 in-flight 清理、watch 通知时序（waiters 在 send 前 subscribe 的窗口——:87-90 先 changed() 再 borrow 的竞态边界）。
- `read_usage_cache_many`（persist.rs:45-65）全仓零测试（仅 lb.rs:426 消费，无任何单测/集成直测）。
- `fetch_and_store` 的 `UsageError::Database` 传播（persist.rs:133-135）无单测（写失败形态依赖真实 DB 故障，难造；至少可注写闭包）。

### 07-05 probe_boundary_providers 与并发刷新路径零单测（P3，测试覆盖）

persist.rs 测试区（315-651）覆盖：余额耗尽停用/恢复（429）、并发 upsert（459）、无法判定保持原状（483）、subscription_usable 三例（516-554）、缓存读写往返与过期（556）、刷新失败点名日志（592）。**probe_boundary_providers 无任何单测**——候选过滤（billing_mode/manual-failure 排除/未开启跳过/无缓存跳过）、Skipped/Failed/成功三分支、recover_quota/disable_for_quota 联动全凭集成 4 场景（provider_boundary_probe_integration）间接覆盖；候选过滤矩阵（quota 态×边界、可用态×边界、manual×边界、无缓存×边界 等组合）建议抽纯函数补单测。另：?refresh=1（providers.rs:808）与 cron 并发路径无测试（07-01 修复后补并发断言）。

## 已核验无问题区（避免后续票重复审查）

- **新鲜度判定真单一**：`cache_age_fresh/cache_age_fresh_at`（persist.rs:68-79）全仓唯一实现——DB 单读（read_usage_cache）、批读（read_usage_cache_many）、mem 读（read/read_many）全部复用；grep 无第二份拷贝。fetched_at 语义=真实抓取时刻（write_usage_cache 用 data.fetched_at 而非 now，persist.rs:91；mem store 同源），两缓存口径一致。
- **原子 upsert（E7 已修项核验无回归）**：write_usage_cache 单语句 `ON CONFLICT(provider_id) DO UPDATE`（persist.rs:83-108），无 find→insert 两段竞态；并发 4 写单测锁定（:459-481）；落库失败 `UsageError::Database` 如实传播不静默（:133-135，注释明示 E7 语义）。
- **级联与幂等**：disable_for_quota/recover_quota 条件更新（enable=true / reason=quota 守卫）+ rows_affected 判定幂等（availability.rs:148-211）；manual/failure 态不被额度刷新触碰（availability.rs:471 测试锁定 quota_disable_never_touches_manual_or_failure、:497 recover_only_quota_state）；级联分层语义（cascade_disabled 标记，恢复只动带标记条目）在 set_items_enabled 单点（:89-143）；recover_probe 乐观锁（updated_at CAS，:302-336 测试锁定 :640）。
- **谓词单源**：subscription_usable/balance_usable/worst_window/has_low_remaining_window/remaining_percent_value 全在 types.rs 单源；usage_rank 只消费访问器（usage_rank.rs:54-96 用 worst_window/remaining_percent_value，注释「排序与判定共用同一访问器」）；apply_usage_gate 用 usable_for_billing_mode；边界探活用 has_low_remaining_window——门控/LB/探活三消费方同一谓词族，无第二份推导。分层读侧判定（availability::traffic_available=选路可用 vs types 可用判定=用量可用）职责清晰（availability.rs:49-57 注释）。
- **恢复双通道自洽**：usage_refresh = refresh_all_usage（含 apply_usage_gate：usable→recover_quota 解除 quota 态）→ probe_boundary_providers（同 handler 顺序执行，读到的缓存必新鲜，persist.rs:236-237 注释）；窗口回升离开 (0,1) → gate 恢复；边界内 → 实测成功恢复/失败停用——两条通道与 ADR-0010 语义一致，无重叠冲突（先 gate 后 probe，probe 不重复处理已 usable 的）。
- **缓存失效面**：provider 更新/删除双缓存同步失效（providers.rs:534-537/685-688：invalidate_usage_cache + usage_mem.invalidate 成对调用）；mem invalidate 同时清 in-flight（mem_cache.rs:59-62）防悬挂。
- **LB 三层读路径**：mem → DB IN 批读 → 单飞抓取（lb.rs:400-455），命中免 DB 往返、批读一次往返、抓取有单飞+失败不回填（fail-open 语义：None 视为无用量不剔除，与「查不到不剔除」口径一致）；每请求零额外 DB 往返。
- **性能形态**：refresh JoinSet + Semaphore(4) 限并发（persist.rs:155-165）；mem 表有界（provider 数）；upsert 单语句；handler try_lock 防自叠；全量刷新 5 分钟周期 × 10 分钟 TTL 使 LB 第三层命中窗口小。

## 性能/内存轮结论

无 P1/P2。正向：三层读路径命中率设计合理（mem/DB 覆盖 10 分钟窗，真实抓取仅冷路径）；批读/批抓取；upsert 单语句；进程内 mem 有界。P3 级：07-01（失败无负缓存——持续故障期每请求真实抓取 + 单请求可被厂商 15s 超时拖住，DB 故障期含已计费厂商调用放大）、07-03（探活顺序执行最坏时长）。结论：形态适合当前规模，无需结构性改动；07-01 的负缓存/入口单飞是唯一值得实施批优先的项（含运维成本与厂商调用费两方面）。
