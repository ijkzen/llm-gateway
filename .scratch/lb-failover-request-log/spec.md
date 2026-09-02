# Spec — LB 降级失败落 request 表

Status: ready-for-agent

## Problem Statement

负载均衡（LB）过程中，当虚拟模型的成员 A 失败并降级重试成员 B 时，A 的失败只在
服务端 tracing 日志中留下一条 warn 记录，请求日志表（`request` 表）与数据面板都
看不到这次失败。只有最终成功者或全部成员失败后才落一行。用户无法在请求日志页与
数据面板中追溯「哪些供应商在降级过程中失败、为什么失败」。

## Solution

failover 循环中，每次成员失败且将要降级重试下一个成员时，也向 `request` 表写入
一行失败记录（`success=false`，`fail_reason` 记具体原因）。这些降级失败行与最终
结果行共用同一次客户端请求的 `request_id` 前缀，通过 `-N` 后缀唯一化主键，并在
数据面板统计中正常计入。

## User Stories

1. 作为管理员，我希望在请求日志页看到「降级过程中失败的成员」行（success=false、
   带具体失败原因），以便判断哪些供应商/模型在上游出问题时被跳过。
2. 作为管理员，我希望同一客户端请求的多次成员尝试能用 `request_id` 关联起来
   （原始 id + 尝试序号后缀），以便还原完整的降级链。
3. 作为管理员，我希望数据面板的成功率、失败诊断、失败趋势与按供应商失败统计能
   如实反映降级失败，以便在面板上直接发现「某供应商持续失败导致降级」。
4. 作为管理员，我希望密钥解密失败、请求构造失败这类本地失败（未发出网络请求）
   同样落库，以便完整记录降级原因，不留盲区。
5. 作为管理员，我希望 fallback=0（直接失败）与最后一个成员失败的既有行为不变
   （原始 request_id 落一行），以免破坏现有日志/统计语义。
6. 作为开发者，我希望无需 schema 迁移即可实现（request_id 加后缀即可唯一化），
   以便用最小改动落地。

## Implementation Decisions

- **修改模块**：`src/proxy/mod.rs`（`forward_chat` 的 failover 循环）。
- **复用现有函数**：`record_failure`（同步函数，内部 spawn 异步 insert）——不新建
  写库路径。4 个降级分支共用循环内局部闭包 `record_degraded(message, ttft_start_ms)`
  折叠重复的 10 参调用（仅 `ttft_start_ms` 不同）。
- **唯一化规则**：降级失败行 `request_id = format!("{request_id}-{N}")`，N 为尝试
  序号从 1 起（循环 `index` 为 0 起，故 N = index + 1）。非降级直接失败分支
  （`!has_more` 或 fallback=0）、全败兜底、成功行保持原始 `request_id`。
- **落库时机**：循环内 4 个「`retry_enabled && has_more` 则 continue」的失败分支
  （密钥解密失败、请求构造失败、上游调用失败、上游返回可重试 ≥400）各补一行
  `record_failure` 调用，字段与同分支非降级路径一致（stream=false、ttft=None、
  ttft_start_ms=start_time 或 reply.start_at_ms），仅 `request_id` 带后缀。
- **无 schema 变更**：不新增列、不迁移（生产 schema 到 16 号段，避开撞号）。
- **无前端改动**：请求日志页直接展示行，数据面板聚合自动计入 success=false 行。
- **全败无汇总行**：全败时每个成员尝试各落一行（最后一个用原始 id），循环后兜底
  `record_failure` 保留（不可达路径），不加额外汇总行。

## Testing Decisions

- **接缝**：现有 `tests/proxy_integration.rs` 的本地 mock 上游 + `wait_for_records`
  断言（HTTP 行为级），复用 `send_chat`/`seed_provider`/`common_setup_with_member`
  等辅助函数，不碰内部函数。
- **改现有测试**：`failover_retries_next_member_on_429` 断言从 1 行改为 2 行，
  并补断言：A 行 `success=false`、`request_id` 以 `-1` 结尾、`fail_reason` 为
  「rate limited」；B 行 `success=true` 且为原始 id。该测试的
  `load_balancing_strategy` 从 3（Random）改为 2（RoundRobin）：随机序会把成功
  成员排前导致 A 不被尝试，新断言（等 2 行）下必挂；确定序 A→B 才能稳定断言
  降级失败行。
- **新增测试**：
  - 全败场景（A、B 均 429，RoundRobin）：落 2 行均 `success=false`，A 行 `-1`
    后缀、B 行原始 id。
  - fallback=0 直接失败：既有 `fail_directly_returns_upstream_error_and_records`
    已覆盖（断言仍只落 1 行原始 id），确认不变仍绿，不新增重复测试。
- **测试基建（超出 spec 原接缝的正当改动）**：`tests/common/mod.rs` 的
  `setup_db_and_scheduler` 从 `sqlite::memory:` 改为临时文件库（共享
  OnceLock 目录 + 每连接唯一文件）。原因：`db::connect` 连接池
  `max_connections(5)` 下，内存库每个连接是独立数据库，`record_failure` 的
  异步 spawn insert 与测试查询可能落到不同连接而互相不可见（2 行断言在并发
  全量跑时稳定 flaky）。文件库使全部连接共享同一数据库，根治该隐患；影响所有
  走 `setup_db_and_scheduler` 的集成测试，但语义不变（每测试独立库文件）。
- **测试什么**：只断言外部可见行为（request 表行数、success、request_id 后缀、
  fail_reason、provider/model），不断言内部调用顺序。
- **既有断言影响检查**：现有测试用 `wait_for_records(&db, 1)` 的地方，若场景不含
  降级（单成员成功/失败）则不受影响；failover 相关测试需要逐一点查。

## Out of Scope

- 前端请求日志页的分组/标记「降级」行（行已可见，无需 UI 改动）。
- 失败重试决策（哪些状态可重试、降级策略判定）——保持现状。
- request_id 后缀的展示层高亮/跳转。
- 任何 schema 迁移或 request 表新列。
- LB 决策日志改动。

## Further Notes

- 降级失败行计入数据面板是用户拍板口径：request 表语义本就是「每次转发的指标
  记录」，一次降级 N 次会产生 N 行失败，成功率/失败诊断会如实变差——这是期望
  行为而非缺陷。
- 生产环境 request 表已累积数据，新行只是多写，对既有行无影响。
