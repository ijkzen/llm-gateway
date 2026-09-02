# REQUIREMENTS — LB 降级失败落库

## 原始请求（逐字）

> 负载均衡过程中失败降级的请求也要记录到请求日志表里

## 背景

`src/proxy/mod.rs::forward_chat` 的 failover 循环中，中间成员失败且可重试时
（`retry_enabled && has_more` 分支），只打 `tracing::warn` 日志就 `continue`，
**不落 request 表**；只有最终成功者（`dispatch_success`）或全部成员失败后
（`record_failure`）才落一行。导致请求日志页与数据面板看不到「谁在降级过程中失败、
为什么失败」。

## 范围

- 在 failover 循环的 4 个降级 continue 分支（密钥解密失败、请求构造失败、上游调用
  失败、上游返回可重试 4xx/5xx）各补一行 `record_failure` 落库。
- 复用现有 `record_failure`（同步函数，内部 spawn 异步 insert），不新建函数。

## 共识（grilling 确认）

1. **主键唯一化**：request_id 加后缀。降级失败行用 `{request_id}-{N}`，N=尝试序号
   从 1 起（第一个失败成员为 `-1`）。无需 schema 变更（不加列、不迁移）。
2. **统计口径**：降级失败行（`success=false`）计入数据面板全部统计（成功率、失败
   诊断、失败趋势、按供应商失败率等）。request 表语义本就是「每次转发的指标记录」，
   降级失败是真实事件。
3. **记录范围**：全部失败路径落库，含未真正发出网络请求的本地失败（密钥解密失败、
   请求构造失败），fail_reason 记具体原因。
4. **失败行 ID 规则**：仅降级失败行带 `-N` 后缀；非降级直接失败（fallback=0 或最后
   一个成员失败，`!has_more` 分支）、全败兜底、成功行保持原始 request_id。全败时
   循环内每行均已落库，不再加「汇总行」。

## 非目标（ponytail 削减）

- 无 schema 变更、无迁移。
- 无前端改动：行自动出现在请求日志页，统计自动计入。
- 无「汇总行」：全败时每个成员尝试各落一行。
- 不区分降级失败与最终失败的展示层标记（fail_reason 已含具体原因，success 与
  request_id 后缀可区分）。
- 不新增 LB 决策日志相关改动（现有决策日志保持）。

## 验收口径

- 一次请求 A 成员失败（可重试）→ B 成功：落 2 行，A 行 `success=false` 且
  `request_id=<uuid>-1`，B 行 `success=true` 且 `request_id=<uuid>`。
- 一次请求 A 失败 → B 失败（全败）：落 2 行，A 行 `<uuid>-1`、B 行 `<uuid>`，
  均 `success=false`。
- fallback=0 直接失败：仍只落 1 行原始 request_id（行为不变）。
- 数据面板失败统计计入上述降级失败行。
