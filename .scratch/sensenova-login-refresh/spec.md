# Spec — SenseNova 登录自愈（refresh_token 失效回退登录）

标签：`ready-for-agent`
来源调研：`.scratch/sensenova-login-refresh/research.md`（纯 API 链路已实测跑通）。

## Problem Statement

SenseNova 用量抓取依赖 `extra.refresh_token`。refresh_token 会因控制台退出登录、改密、并发刷新轮换
断链而失效（`invalid_grant`），此时用量卡片报「用量查询凭据无效或已过期」，必须人工回浏览器重做 PKCE
提取并粘贴新 token。用户希望 `extra` 增加 `username`/`password`：用量抓取优先用 refresh_token，
失效时自动用账号密码登录换取新 refresh_token 并回写数据库，实现自愈。

## Solution

sensenova fetcher 改为仿 Krill 的 JWT 自愈状态机：refresh_token 有效 → 既有续期查询；refresh_token
缺失或明确鉴权失败 → 用 username/password 走纯 API 登录（authorize→JWKS→JWE→nova/login→redirect→code→
oauth2/token）换取新 refresh_token → 写回 provider extra → 重试一次续期查询。模板与 UI 把 refresh_token
降为后端派生隐藏字段，用户只维护 username/password。

## User Stories

1. 作为管理员，我在商汤供应商配置了账号密码后，即使 refresh_token 过期，用量卡片也能自动恢复，无需我再回浏览器提取 token。
2. 作为管理员，我不希望账号密码每次用量查询都被使用——只在 refresh_token 明确失效时才登录。
3. 作为开发者，我希望登录失败（密码错误/账号锁定）能给出与现在一致的「用量查询凭据无效或已过期」提示，不引入新错误形态。
4. 作为已有商汤供应商的管理员，我希望模板回填自动给我的供应商补上 username/password 空字段，刷新 token 仍保留可用。
5. 作为前端用户，我希望编辑商汤供应商时只看到 username/password 输入，refresh_token 由系统维护、不展示。

## Implementation Decisions

### 后端

- **sensenova fetcher 状态机**（`src/usage/fetchers/sensenova.rs`）：
  - 签名改为 `fetch_sensenova(http, creds, ctx)`，`ctx: &SensenovaContext`（新增于 `fetchers/mod.rs`，
    仿 KrillContext：`db` + `provider_id`），供登录路径写回 refresh_token。
  - `refresh_token` 分支保留现有逻辑，仅调整错误语义：续期响应 HTTP 401/403 → `Auth`；
    200 但 `{"error":"invalid_grant"}` 或解析不出 `access_token` → `Auth`（触发登录）。
  - 登录分支：
    ```
    username = creds.require("username")?; password = creds.require("password")?;
    challenge = authorize_step(...)?;         // 拿 login_challenge（带 cookie 会话）
    pubkey    = jwks_step(...)?;              // kid=public:hydra.openid.id-token
    jwe       = jwe_encrypt(pubkey, password)?; // 5 段：RSA-OAEP(SHA1) + A256GCM，tag 独立
    redirect  = nova_login(username, jwe, challenge)?; // 400 incorrectUsernameOrPassword → Auth
    code      = follow_redirect(redirect)?;   // 同 cookie 会话，consent 自动，取 ?code=
    tokens    = token_exchange(code, verifier)?; // oauth2/token 换 refresh_token
    write_back_refresh_token(ctx.db, ctx.provider_id, &tokens.refresh_token).await?;
    // 用新 refresh_token 走既有续期+查询一次
    ```
  - 解析拆纯函数单测：`login_reason(reply)`（是否需要登录）、`parse_login_redirect(body)`、
    `parse_token_response(body)`、`parse_error_reason(body)`（incorrectUsernameOrPassword）。
  - HTTP 层：需 cookie jar + 自动重定向。现有 `UsageHttp` 是 reqwest client 无 cookie 存储、`send`
    手动构造不跟随重定向（reqwest 默认 follow 但无 cookie）。新增一个小型 login 子客户端或给 `UsageHttp`
    增加 cookie 支持（用 reqwest 的 `cookie_store` feature 或手动 Set-Cookie 传递）。倾向给 `UsageHttp`
    开 `cookie_store(true)` + `redirect(Policy::limited(8))`，登录步骤复用现有 `get/post_form`。
  - RSA-OAEP：新增 `rsa = "0.9"` 依赖（`rsa::RsaPublicKey::new` + `oaep` padding SHA-1）；JWKS JSON
    解析用 serde_json 手写（取 n/e → `RsaPublicKey`）。JWE 5 段拼接用现有 base64/aes-gcm。

- **分发/上下文**（`src/usage/mod.rs` + `fetchers/mod.rs`）：
  - `Fetcher::Sensenova` 分支需 db/provider_id → 仿 Krill：`query_provider_usage` 特判 Sensenova，
    构造 `SensenovaContext { db, provider_id }` 传入；登录内写回直接 `write_back_refresh_token`。
  - 现有 `rotated_refresh_token` out-param 写回通道保留给「refresh_token 有效」路径的轮换写回。

- **模板种子**（`src/provider_template/seed.rs`）：SenseNova extra →
  `{"refresh_token":"", "username":"", "password":"", "usage":true, "usage_type":1}`。

- **路由校验**（`src/routes/providers.rs::validate_extra`）：豁免清单加 `"refresh_token"`。

- **回填**：复用既有模板 backfill（insert 分支向同 host provider 合并缺失键，只补缺不覆盖）——
  确认 `provider_template/mod.rs::backfill_provider_extra` 覆盖即够；不改逻辑。

### 前端

- `ProviderEditDialog.tsx::editableExtraKeys` 排除 `refresh_token`（现排除 usage/usage_type/jwt）。
- 保存 payload 的 spread 合并已保留隐藏键（refresh_token 随模板 extra 原样保留），无需改。
- 无新增 i18n 键（沿用原始 key 作 label；password 已按 key==="password" 掩码）。

## Testing Decisions

- **单元**（sensenova.rs mod tests）：登录判定/响应解析纯函数（login_reason、redirect 提取、
  token 响应解析、错误 reason 映射 Auth）；JWE 5 段结构（用测试 RSA 密钥对加密→解密回明文断言）；
  不触发登录的条件（网络/5xx/解析失败）。
- **集成**（tests/provider_usage_integration.rs，`LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 本地 mock）：
  mock 端点覆盖六步：`/oauth2/auth`（302 login?login_challenge=）、`/.well-known/jwks.json`、
  `/iam/authn/v1/auth/nova/login`（redirect 字段）、consent 跳转、`/oauth2/token`。
  场景：
  - refresh_token 有效 → 正常续期+查询（既有用例保持通过）。
  - refresh_token 失效（invalid_grant）→ 登录 mock → 新 refresh_token 写回（断言 DB 加密 extra）→ 重试成功。
  - refresh_token 缺失（只有 username/password）→ 直接登录引导写回。
  - 登录失败（nova/login 返回 incorrectUsernameOrPassword）→ Auth → 400「用量查询凭据无效或已过期」。
  - 网络错误不触发登录（跳过 login mock 调用断言）。
- **前端**：provider-edit-dialog.test.tsx 断言 SenseNova 编辑时 refresh_token 不渲染、
  username/password 可见且 password 为 password 输入。
- 全量门禁：cargo fmt / clippy -D warnings / cargo test --all-targets / pnpm lint / pnpm vitest run。

## Out of Scope

- TOTP / 短信验证码 / 图形验证码处理。
- access_token 二级缓存。
- `UsageError` 新变体与新错误文案。
- 数据库迁移。
- 登录频率节流/账号锁定的自动规避。
