# 02: 全败与直接失败场景回归测试

**What to build:** 补全 failover 落库的边界场景测试，锁定三种行为：全部成员失败时每个成员尝试各落一行（最后一个成员用原始 request_id）；fallback=0（直接失败）仍只落一行原始 request_id（行为不回归）；单成员成功场景不受影响。

**Blocked by:** 01: 降级失败成员落 request 表

**Status:** ready-for-agent

- [x] 新增测试：A、B 两成员均返回 429（fallback=1，RoundRobin）→ 落 2 行均 success=false，A 行 request_id 以 `-1` 结尾、B 行原始 request_id；响应为 429（取最后失败）。
- [x] fallback=0 直接失败：既有 `fail_directly_returns_upstream_error_and_records` 覆盖（断言仍只落 1 行原始 id），确认不变仍绿。
- [x] 测试基建：`setup_db_and_scheduler` 改临时文件库（根治内存库多连接隔离导致 2 行断言 flaky）。
- [x] 全量测试绿：`cargo test`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check`。
