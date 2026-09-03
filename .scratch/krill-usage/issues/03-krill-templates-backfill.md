# 03: Krill 模板与历史回填

**What to build:** 管理员可以选择按量或订阅 Krill 模板创建 Provider；升级时三个 Krill host 下的既有 Provider 会幂等补齐用量凭据结构并按原付费模式校正类型，敏感数据继续加密且用户显式配置不被覆盖。

**Blocked by:** 02: Krill 用量与 JWT 自愈.

**Status:** completed

- [x] seed 包含同一国内极速 Base URL 的按量与订阅两个唯一模板，协议、billing_mode、usage_type 和 extra 正确。
- [x] 同 host 模板匹配同时返回两项，三个 Krill host 均被用量分发识别。
- [x] 每次启动幂等扫描三个 host 的历史 Provider，不依赖模板首次插入。
- [x] 回填补缺 email/password/jwt；缺失 usage 才设 true；usage_type 始终按既有 billing_mode 校正。
- [x] 已有凭据与显式 usage=false 保留，billing_mode 不改变，extra 继续加密。
- [x] 解密失败不覆盖原数据，并产生明确可诊断结果。
- [x] 双模板、三 host、两种模式、保留规则和重复运行均有数据库测试且先红后绿。

## Completion notes

红测确认同 host 没有 Krill 模板且历史行未补凭据；实现后 Krill 专项 2 个测试和 provider_template 全部 13 个测试通过。回填复用启动模板同步，不新增 schema migration。
