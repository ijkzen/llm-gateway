# 01: 降级失败成员落 request 表

**What to build:** 负载均衡降级时，中间成员失败（可重试降级）的尝试也写入请求日志表。用户在请求日志页能看到失败降级的成员行（success=false、带具体失败原因），且该行以 `request_id-N`（N 为尝试序号从 1 起）唯一化主键，与最终结果行（原始 request_id）可通过前缀关联。数据面板失败统计自动计入这些行。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [x] `src/proxy/mod.rs` failover 循环 4 个「retry_enabled && has_more 则 continue」分支（密钥解密失败、请求构造失败、上游调用失败、上游返回可重试 ≥400）各补一行 `record_failure` 调用（循环内局部闭包 `record_degraded` 折叠），`request_id = {原始 id}-{index+1}`，字段与同分支非降级路径一致。
- [x] 现有 `failover_retries_next_member_on_429` 测试断言改为 2 行：A 行 success=false 且 request_id 以 `-1` 结尾、fail_reason 为「rate limited」；B 行 success=true 且为原始 request_id（策略改 RoundRobin 保证 A→B 确定序）。
- [x] 全量测试绿：`cargo test`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check`。
