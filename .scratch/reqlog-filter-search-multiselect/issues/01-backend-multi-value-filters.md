# 01: 请求日志接口过滤参数支持逗号分隔多值

**What to build:** 管理员在请求日志页能按多个虚拟模型/供应商/上游模型/API Key 一次过滤——`GET /api/request-logs` 的 `vmId`/`providerId`/`modelId`/`apiKey` 接受逗号分隔多值（如 `vmId=1,2,3`），生成 SQL `IN` 过滤；单值形式天然兼容；空串/空段忽略；参数缺失 = 不过滤。`success` 保持布尔单值。参数化绑定，无注入面。

**Blocked by:** None (can start immediately).

**Status:** done

- [x] 逗号分隔多值参数返回的行是各值的并集（IN 语义）
- [x] 单值参数行为与改造前一致（集成测试回归）
- [x] 缺省/空串 = 不过滤；含空段的脏值（如 `1,,2`）忽略空段
- [x] `cargo test --all-targets` 全绿（含扩展后的 request_logs 集成测试）
