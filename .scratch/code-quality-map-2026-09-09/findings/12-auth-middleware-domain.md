# FINDINGS · 12 鉴权与中间件域审查（2026-09-10）

范围：`auth/mod.rs`（499 行全读：argon2/会话/Bearer/中间件/回填）、`routes/auth.rs`（263 行全读：status/init/login/logout/me/change-password）、`crypto/mod.rs`（267 行全读含 11 单测）、`middleware/mod.rs`（19 行全读：CORS/Trace/CatchPanic）+ 交叉核对（entity/user 唯一约束、app_settings 双写面、db.rs 索引、routes/mod.rs 层序、auth_integration 6 用例）。方法：域小（1048 行）主代理直接全读，无子代理。清单模式：不改代码。**本票无 P1/P2、无需拍板项**（默认解全部明确；CORS 为 AGENTS.md 已登记风险不重复拍板）。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 12-01 | P3【已修复 2026-09-10】 | 逻辑/竞态 | init 为 check-then-act：并发双初始化（不同用户名）可建两个用户，单用户假设破坏（username UNIQUE 只挡同名） |
| 12-02 | P3【观察级保持现状 2026-09-10】 | 安全观察 | 会话 Cookie 无 `Secure` 属性：HTTPS 部署下若经 http:// 访问会明文带 cookie（自部署内网 HTTP 是常态，记观察） |
| 12-03 | P3【已修复 2026-09-10】 | 逻辑/边界 | `crypto::mask` 用 bytes 判长、chars 切片：多字节密钥（如 4 个 CJK 字符=12 bytes）head/tail 重叠，整体泄漏（实际 API key 均 ASCII，窄边界） |
| 12-04 | P3【已修复 2026-09-10】 | 逻辑/口径 | `decrypt_or_passthrough` 对「带 enc:v1: 前缀但解不开」（密钥丢失/轮换）的密文也原样透传当明文用——只对无前缀历史明文正确；与写侧「解密失败必须 Err」教训不对称 |
| 12-05 | P3【已修复 2026-09-10】 | 逻辑/边界 | logout 不在公开清单：过期会话下 POST logout 被中间件 401，服务端清不掉 cookie（前端 401 跳登录兜底，语义毛刺） |
| 12-06 | P3【已修复 2026-09-10】 | 逻辑/边界 | `/v1/messages` 前缀匹配无边界：`/v1/messagesXYZ` 也获 x-api-key 授权口径与 Anthropic 错误格式（同 key 校验，无安全问题） |
| 12-07 | P3【已修复 2026-09-10】 | 简洁 | auth_middleware 持 State 却走 `AppSettings::process_global` 旁路取 lang；`SESSION_TTL_SECS` 常量在 login_response 被字面量 `7*24*3600` 重复 |
| 12-08 | P3【观察级保持现状 2026-09-10】 | 性能微观察 | 每个 /api 请求 session+user 两次点查，可 JOIN 合一（SQLite 本地，量微） |
| 12-09 | P3【已部分修复 2026-09-10】 | 测试覆盖 | 缺口六类：并发双 init / 过期会话自动删除 / x-api-key 入站鉴权 / cookie 属性断言 / auth 端点英文分支 / logout 无会话 401 行为 |

## 各条证据

### 12-01 init check-then-act 竞态（P3，竞态）【已修复 2026-09-10】

routes/auth.rs:102-112 先 `Entity::find().count()` 判空，:128 再 insert。两个并发 init 请求（不同用户名）可同时通过 count==0 检查并各自插入成功——`user.username` 有 UNIQUE（entity/user.rs `#[sea_orm(unique)]`）只挡同名。后果：单用户系统出现两行用户，两个会话各自有效、change-password 各改各的。窗口=首次初始化瞬间，自部署场景一次初始化，P3。默认解：check-then-act 改为「直接 insert + 唯一约束兜底」（如 user 表加单行约束或以固定 id=1 upsert），或初始化接口加进程内 OnceLock 门。

### 12-02 会话 Cookie 无 Secure 属性（P3，安全观察）【观察级保持现状 2026-09-10】

login_response（routes/auth.rs:188-189）与 clear_session_cookie（auth/mod.rs:162-164）均为 `HttpOnly; SameSite=Lax; Path=/; Max-Age=...`，无 `Secure`。生产经 FRP TLS（gateway.ijkzen.cn）时若用户以 http:// 访问，浏览器会明文携带会话 cookie；SameSite=Lax 已挡跨站携带。自部署常客走内网 HTTP（加 Secure 反而登录不了），记观察项。默认解：图后如要收紧，按 `APP_ENV=prod` 或显式环境变量条件性追加 Secure。

### 12-03 mask 多字节重叠泄漏（P3，边界）【已修复 2026-09-10】

crypto/mod.rs:122-134：`bytes.len() <= 7` 用字节判长，`chars().take(3)`/`rev().take(4)` 用字符切片。4 个 CJK 字符（12 bytes）通过长度检查，head=前 3 字、tail=后 4 字 → `甲乙丙****甲乙丙丁`，全部内容泄漏。实际 API key 均 ASCII 不触发，P3 边界。默认解：统一按 chars 计数判长。

### 12-04 decrypt_or_passthrough 对解不开的密文透传（P3，口径）【已修复 2026-09-10】

crypto/mod.rs:112-114：`decrypt` 失败即原样返回存储值。两类失败混在一起：①无前缀历史明文（透传正确，设计本意）；②带 `enc:v1:` 前缀但密钥缺失/轮换/损坏（透传错误——密文被当明文 key 发上游，401 + failover 兜底；展示侧 mask 泄漏密文首尾各数字符，无明文泄漏）。写侧教训（extra 写回解密失败必须 Err）在写路径已守住，读侧这里是不对称的 fail-open。默认解：`is_encrypted(stored)` 为 true 而 decrypt 失败时返回错误/哨兵（如空串+error 日志），仅无前缀值透传。

### 12-05 logout 需会话、过期会话清不掉 cookie（P3，边界）【已修复 2026-09-10】

`/api/auth/logout` 不在 auth_public 清单（auth/mod.rs:263-266）→ 会话过期时 logout 请求被中间件 401（auth/mod.rs:293-295），到不了 logout handler，服务端 revoke 与 Set-Cookie 清除都不执行；浏览器内 cookie 残留至自然过期（已无效，无安全后果；前端 401 跳登录兜底）。语义上 logout 宜幂等公开。默认解：logout 加入公开清单（handler 内部本就自行 extract_cookie+revoke，容忍无 cookie）。

### 12-06 /v1/messages 前缀无边界（P3，边界）【已修复 2026-09-10】

auth/mod.rs:275 `path.starts_with("/v1/messages")`（授权口径）与 :343（错误格式选择）：`/v1/messages-foo` 这类不存在路径也按 messages 处理——允许 x-api-key 头、错误体按 Anthropic 形状。两种凭证走同一 api_key 校验，无安全放大；未知路径最终落 SPA fallback（11-18 已记）。默认解：精确匹配或加 `/` 边界（`== "/v1/messages" || starts_with("/v1/messages/")`）。

### 12-07 lang 取值旁路与 TTL 字面量重复（P3，简洁）【已修复 2026-09-10】

- auth_middleware 持有 `State(state)` 但错误分支经 `current_lang()`（auth/mod.rs:169-174）走 `AppSettings::process_global` 旁路，同进程存在 state.settings / process_global / LANG_SYNC 静态（app_settings.rs:273）三个同源表述；双写有注释自觉（app_settings.rs:253 注释「静态缓存与 inner 保持一致」），非缺陷但可收。默认解：中间件直接用 state.settings.lang()，current_lang 仅留无 State 场景。
- login_response（routes/auth.rs:187）clamp 上限写字面量 `7 * 24 * 3600`，与 `SESSION_TTL_SECS`（auth/mod.rs:31）重复；改常量不改 clamp 会静默漂移。默认解：引用常量。

### 12-08 /api 每请求双点查（P3，性能微观察）【观察级保持现状 2026-09-10】

session_user（auth/mod.rs:100-118）= session 主键点查 + user 主键点查，每个 /api 请求两次 DB 往返；可 JOIN 合一。SQLite 本地微秒级，量微，记观察。/v1 侧已是单条索引查询（key_hash 有索引，db.rs:264）。

### 12-09 测试缺口（P3，测试覆盖）【已部分修复 2026-09-10】

现有：auth_integration 6 用例（init 流程/非法输入/登录守卫/改密踢会话/登出吊销/Bearer 必需）+ auth 模块内 5 单测（哈希往返/token 摘要/随机性/cookie 解析/Bearer 大小写）+ crypto 11 单测（轮换/篡改/无密钥/掩码）。缺口：
- T1 并发双 init（12-01）：双请求并发初始化，断言只有一行用户。
- T2 过期会话路径：构造过期 session 行 → 请求返 401 且行被顺带删除（session_user:111-116 无测试）。
- T3 x-api-key 入站鉴权：/v1/messages 用 x-api-key 头放行、其余 /v1 端点不认该头（tests 内现有 x-api-key 命中全是出站头透传，非入站鉴权）。
- T4 Set-Cookie 属性断言（HttpOnly/SameSite=Lax/Max-Age 与 7 天上限）。
- T5 auth 端点英文分支（i18n_integration 只覆盖 providers/settings）。
- T6 logout 无会话/过期会话的 401 行为锁定（12-05 改公开后改断言 200）。

## P3 实施批（2026-09-10）

- **12-03 已修复**：`crypto::mask` 的长度判定改为按字符数（原先按字节判长、按字符切片——4 个汉字 = 12 字节会通过长度检查从而完整回显）；新增 `mask_counts_chars_not_bytes` 锁定。
- **12-04 已修复**：`decrypt_or_passthrough` 对「带 `enc:v1:` 前缀但解不开」的值返回空串 + warn（原先把密文当明文交给上层 = 拿密文去鉴权的 fail-open）；仅无前缀历史明文走透传。`decrypt_or_passthrough_never_fails` 随批更新语义。
- **12-05 已修复**：`/api/auth/logout` 加入公开路径清单——会话过期时不再被 401 拦在门外，服务端 revoke 与清 cookie 都能执行（handler 本就容忍无 cookie）。回归 `cookie_attributes_and_public_logout`。
- **12-06 已修复**：`/v1/messages` 前缀加路径边界（`== "/v1/messages" || starts_with("/v1/messages/")`），两处（x-api-key 授权口径与错误信封形状）；回归 `v1_messages_prefix_requires_boundary`。
- **12-07 已修复**：登录响应的会话时长 clamp 改引 `auth::SESSION_TTL_SECS`（字面量重复消除）；测试断言 Max-Age 等于该常量（±1s 舍入）。
- **12-09 已部分修复**：新增 3 条集成——过期会话 401 且行被顺带清理（`expired_session_is_rejected_and_row_purged`）、Set-Cookie 四属性 + Max-Age 常量（`cookie_attributes_and_public_logout`）、x-api-key 入站鉴权与路径边界（`v1_messages_prefix_requires_boundary`）。并发双 init（T1）已于 12-01 批补齐。
- **12-02 / 12-08 观察级保持现状**：Cookie 无 Secure（自部署常有内网 HTTP，加了反而登录不了；FRP TLS 下线可再收紧）、/api 每请求双点查（SQLite 本地微秒级）。


## 已核验无问题区（避免后续票重复审查）

- **密码面**：argon2id 哈希（hash_password/verify_password）；登录对不存在用户做 dummy 等价校验抹平用户存在性时序（routes/auth.rs:156-166 注释自觉）；密码 6-128、用户名 1-64 校验；init 唯一冲突映射 400。
- **会话面**：256 位 OsRng 随机 token；库只存 SHA-256（hash_token 为主键）；HttpOnly+SameSite=Lax；7 天 TTL 单常量；过期顺带删除幂等（并发双删安全）；改密吊销其他会话保留当前（集成测试锁定）；登出吊销当前。
- **Bearer 面**：key_hash=SHA-256 索引查找（O(1)）；enable 过滤；DB 错误 500 与无效凭证 401 区分；错误格式按端点分 Anthropic（/v1/messages）/OpenAI（其余）双形状；Bearer 大小写两种 scheme 前缀（测试锁定）。
- **守卫面**：公开清单=healthz+status/login/init（auth/mod.rs:263-266）；/api/* 会话、/v1/* Bearer 分流正确；SPA 与未知路径放行在守卫之后；logout/me/change-password 均需会话（12-05 记 logout 边界）；11 票的尾斜杠（11-17）与未知路径 HTML 200（11-18）不重复记录。
- **中间件层序**：create_app 内 fallback→DefaultBodyLimit→auth，apply 再包 Cors→Trace→CatchPanic——CatchPanic 最外层兜住含 auth 在内的全部 handler panic；DefaultBodyLimit 经 extension 作用于 extractor，层序不影响其生效；Trace 默认级别在 RUST_LOG=info 下静默。
- **CORS 现状**：`CorsLayer::permissive()`（middleware/mod.rs:12）为 AGENTS.md 安全节已登记风险；SameSite=Lax 使跨站 POST 不携带会话 cookie；/v1 需持 key 才可调。维持登记口径，不重复拍板。
- **crypto 边界**：AES-256-GCM 每次随机 nonce（重复密文测试锁定）；tag 篡改/密钥轮换/无密钥/短 blob/非 UTF-8 五类错误路径有测试；enc:v1: 前缀识别；空串加解密直通；明文只在未配置密钥时落库且 warn（模块头注释明示的文档化设计）。
- **启动回填**：backfill_api_key_hashes 对解不开的 key 跳过并 warn（该 key 无法用于 /v1 鉴权），不阻断启动。
- **依赖方向**：routes/middleware → auth → entity/crypto/app_settings，无反向依赖；proxy 经 extensions 消费 AuthedApiKey 类型，无 auth 内部细节泄漏。
- **lang 双写面**：process_global（async）与 LANG_SYNC（sync，CatchPanic 用）双写有注释自觉、单 update 路径同步，一致。

## 性能/内存轮结论

无 P1/P2。/v1 鉴权=单条索引查询（key_hash）；/api 鉴权=两次主键点查（12-08 微观察）；argon2 仅在登录/改密/init 触发（每次数百 ms，登录无限流但慢哈希天然限速，暴力破解 6+ 位密码不现实）；derive_key 为单次 SHA-256 廉价；delete_expired_sessions 仅在 login 触发且 session 表极小。结论：鉴权热路径开销可忽略，无需任何结构性改动；唯一可做的是 12-08 的 JOIN 合并（图后顺手项）。

## 实施进度（2026-09-10）

- **12-01 已修复**：`routes/auth.rs::init` 由「先 count 再 insert」改为单条原子语句 `INSERT INTO user (...) SELECT ... WHERE NOT EXISTS (SELECT 1 FROM user)`——条件在写入路径内求值，并发双初始化（不同用户名）由数据库保证恰好一个成功；插入被挡下时按实际状态给出「已初始化」或「同名用户已存在」文案。回归测试 `concurrent_init_creates_exactly_one_user`（并发两请求恰好一 200 一 400，库里 user 计数为 1）。
