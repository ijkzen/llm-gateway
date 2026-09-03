# 02: Krill 用量与 JWT 自愈

**What to build:** Krill Provider 能通过公开用量查询入口获得与其付费模式一致的余额或套餐窗口；系统优先复用 JWT，只在缺失或认证失败时用邮箱密码登录，安全回写新 JWT 并只重试一次。

**Blocked by:** 01: Daily 订阅窗口贯通.

**Status:** completed

- [x] 三条 Krill 模型 API host 都分发到同一 Krill 用量能力，控制台请求固定走 krill-code.com。
- [x] billing_mode=0 返回总余额 primary 及钱包/福利明细，billing_mode=1 返回逐 active 套餐且带套餐名的窗口。
- [x] usd_daily、usd_weekly、usd_monthly、request_count 按 spec 映射；非 active 或缺字段安全降级。
- [x] 有效 JWT 不登录；JWT 为空先登录；HTTP 401/403 与业务 code 401 登录并最多重试一次；其他错误不登录。
- [x] 新 JWT 在重试前严格合并回写到加密 extra，保留其他字段；解密或写回失败不继续。
- [x] TOTP 明确失败；登录与查询复用现有 HTTP/代理链路且不增加依赖或 token 缓存。
- [x] 纯解析与数据库+mock HTTP 集成测试先红后绿。

## Completion notes

红测先证明 Krill host 不受支持，公开查询 seam 随后暴露空 JWT 被通用 Provider 校验误判为必填；修复后 Krill 11 个单测、4 个 mock HTTP 集成测试和 SenseNova 动态凭据回写回归通过。
