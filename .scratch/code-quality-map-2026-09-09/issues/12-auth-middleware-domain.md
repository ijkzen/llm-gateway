# 12 · 鉴权与中间件域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对鉴权与安全域做全量审查：`auth/` 模块（argon2/session 表/中间件/Bearer）、`routes/auth.rs`、`middleware/`（CORS/Trace/CatchPanic）、api_key 鉴权与 `crypto` 的加密边界及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：会话 7 天/改密踢会话、Bearer 查找与 key_hash、初始化流程竞态、CORS 已知风险现状、panic 捕获遗漏面；
- 实现简洁：中间件与路由的重复校验、错误响应映射；
- 测试覆盖：auth_integration 之外缺什么（时序竞态、边界 token）；
- 模块间调用：auth 与 routes/proxy/entity 的依赖方向是否合适（领域逻辑是否泄漏进中间件）。

产出 `.scratch/code-quality-map-2026-09-09/findings/12-auth-middleware-domain.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/12-auth-middleware-domain.md`——9 条全 P3，无 P1/P2、无拍板项。域小（1048 行）由主代理直接全读，无子代理。

**九条 P3**：12-01 init check-then-act 竞态（并发不同名双初始化可建两用户，username UNIQUE 只挡同名；窗口=首次初始化瞬间）/ 12-02 会话 Cookie 无 Secure 属性（HTTPS 部署下 http:// 访问明文携带，自部署内网 HTTP 常态记观察）/ 12-03 `crypto::mask` bytes 判长 chars 切片，多字节密钥 head/tail 重叠整体泄漏（实际 key 均 ASCII 不触发）/ 12-04 `decrypt_or_passthrough` 对「带 enc:v1: 前缀但解不开」的密文也透传当明文（只对无前缀历史明文正确，上游 401+failover 兜底；与写侧「解密失败必须 Err」教训不对称）/ 12-05 logout 不在公开清单（过期会话 logout 被 401 清不掉 cookie，前端兜底）/ 12-06 `/v1/messages` 前缀无边界（messagesXYZ 同获 x-api-key 口径，同 key 校验无安全放大）/ 12-07 简洁两点（auth_middleware 持 State 却走 process_global 取 lang；SESSION_TTL_SECS 在 login_response 被字面量重复）/ 12-08 /api 每请求 session+user 双点查可 JOIN 合一（微观察）/ 12-09 测试缺口六类（并发双 init、过期会话自动删除、x-api-key 入站、cookie 属性断言、auth 英文分支、logout 无会话行为）。

**已核验无问题区**（12 项，详见 findings）：argon2id+登录时序均衡 dummy、会话面（256 位随机 token/库只存 SHA-256/HttpOnly+SameSite=Lax/过期幂等删/改密踢会话有测试）、Bearer 面（key_hash 索引/双错误形状/大小写 scheme）、守卫面与层序（CatchPanic 最外兜 auth、BodyLimit extension 机制不受层序影响）、CORS permissive 维持 AGENTS.md 已登记口径、crypto 五类错误路径测试齐备、backfill 跳过不可解密行、依赖方向无反流、lang 双写有注释自觉。

**性能轮**：无 P1/P2；/v1 单索引查询、argon2 只在登录/改密、鉴权热路径开销可忽略。

**需拍板问题**：无。

Status: resolved
