# 调研 — SenseNova 账号密码登录换取 refresh_token（纯 API 可行性）

结论：**完全可纯 API 自动化**（无验证码、无人工 consent、无浏览器）。已用真实账号端到端跑通
「账号密码 → refresh_token」全链路。本文档记录所有端点/请求/响应事实，供实现参考；不包含真实凭据。

## 背景

现有 `sensenova` fetcher 只认 `extra.refresh_token`（OAuth `grant_type=refresh_token` 续期）。
refresh_token 会因退出登录/换密码/并发轮换断链而失效（`invalid_grant`），失效后只能手工回浏览器
重做 PKCE 提取。目标：`extra` 增加 `username`/`password`，refresh_token 失效时自动用账号密码重新登录换取新 refresh_token 并回写数据库。

## 关键架构事实

SenseNova 控制台登录是 **Hydra OIDC + PKCE** 全流程，分散在三个域：

| 域 | 角色 |
|---|---|
| `platform.sensenova.cn` | 控制台 SPA；OAuth `authorize`/`token` 端点（`client_id=nova`，公开无 secret） |
| `iam.sensecoreapi.cn/iam/authn` | 账号体系 REST API（登录/验证码/注册） |
| `signin.sensecore.cn` | Hydra issuer（`iss`），公开 JWKS |

前端登录页 SPA 在 `/login`，有「手机号登录」与「账号密码登录」两个 tab；账号密码登录
username 规则：6-24 位 `[A-Za-z0-9]`（即**自定义用户名，非邮箱**）。

## 完整登录链路（纯 API，已跑通）

### 1. 发起 OAuth authorize 拿 login_challenge

```
GET https://platform.sensenova.cn/oauth2/auth?client_id=nova&response_type=code
    &redirect_uri=https%3A%2F%2Fplatform.sensenova.cn
    &scope=openid+offline+offline_access
    &state=<任意≥8字符>
    &code_challenge=<S256 PKCE challenge>&code_challenge_method=S256
```
- 302 → `https://platform.sensenova.cn/login?login_challenge=<hex>`。
- 需要 cookie jar 保存 `oauth2_authentication_csrf`（后续跳转同会话）。

### 2. 取 RSA 公钥（加密密码用）

```
GET https://signin.sensecore.cn/.well-known/jwks.json
```
- 取 `keys[]` 中 `kid == "public:hydra.openid.id-token"` 的 JWK（`kty=RSA`，2048/4096）。
- 前端从同一个 JWKS 选同一 kid（`nova_login_jwk_keys` localStorage 即此物）。
- 前端 JS 把它 `{...key, use:"enc", alg:"RSA-OAEP"}` 后当加密公钥用。

### 3. 密码加密为 JWE（**5 段格式，关键**）

前端用 `jose` 库：`new EncryptJWT(password).setProtectedHeader({alg:"RSA-OAEP",enc:"A256GCM"}).encrypt(publicKey)`。

实际产物是 **5 段点分**（非标准 compact 4 段）：
```
<header>.<encryptedKey>.<iv>.<ciphertext>.<tag>
```
- header: `{"alg":"RSA-OAEP","enc":"A256GCM"}`（base64url）
- encryptedKey: RSA-OAEP（SHA-1）加密 32 字节 CEK（`RSA/ECB/OAEPWithSHA-1AndMGF1Padding`，即 OAEP 默认 SHA-1）
- iv: 12 字节（AES-GCM）
- ciphertext: 明文字节（**不含 tag**）
- tag: 16 字节（第 5 段，与 ciphertext 分离）

> ⚠️ 4 段标准 compact（ct 尾含 tag）会导致服务器 `incorrectUsernameOrPassword` —— 服务端解析器
> 期望 tag 独立成段。实测 5 段即登录成功。

### 4. 账号密码登录

```
POST https://iam.sensecoreapi.cn/iam/authn/v1/auth/nova/login
Content-Type: application/json
Origin/Referer: https://platform.sensenova.cn
{"username":"<用户名>","password":"<JWE>","challenge":"<login_challenge>","is_encrypt":true}
```
- **成功 200**：
  ```json
  {"access_token":"","token_type":"","expires":0,"refresh_token":"","expire_time":null,
   "redirect_to":"","redirect_uri":"",
   "redirect":"https://platform.sensenova.cn/oauth2/auth?client_id=nova&code_challenge=...&login_verifier=<hex>&redirect_uri=...&response_type=code&scope=openid+offline+offline_access&state=...",
   "id_token":"","tenant_list":[]}
  ```
  → 取 `redirect` 字段（无 redirect_uri/redirect_to 时为空）。
- **失败 400**：
  ```json
  {"code":3,"message":"InvalidArgument","details":[
    {"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"incorrectUsernameOrPassword","domain":"iam","metadata":{}},
    {"@type":"type.googleapis.com/google.rpc.LocalizedMessage","locale":"zh-CN","message":"账号或密码错误，请检查输入。再输错4次该账号将被锁定15分钟"},
    {"@type":"type.googleapis.com/sensetime.core.higgs.error_detail.v1.LogInfo","log_id":"...","track_id":"..."}]}
  ```
  - `reason == "incorrectUsernameOrPassword"` 即账号/密码错误 → 映射鉴权失败。
  - 注意错误提示：**连错 4 次锁定 15 分钟**（登录不能狂试）。
- 其它 `details[]` 类型：`captcha_required` 等（本账号实测未触发；保险起见仅处理上述）。

### 5. 跟随 redirect 直到拿到 code

用**同一 cookie jar** 跟随（自动处理 302/303，需保留 `oauth2_authentication_csrf` 与登录后新 cookie）：
```
redirect → 302 https://iam.sensecoreapi.cn/iam/authn/v1/auth/consent?consent_challenge=<hex>
         → 302 https://platform.sensenova.cn/oauth2/auth?client_id=nova&code_challenge=...&consent_verifier=<hex>&...
         → 303 https://platform.sensenova.cn/?code=<code>&scope=openid+offline+offline_access&state=<state>
```
- consent 步骤**自动批准**（无人工页面、无额外 POST）。
- 最终 URL query 里的 `code` 即授权码（用开始时的 `state` 校验）。

### 6. code 换 refresh_token

```
POST https://platform.sensenova.cn/oauth2/token
Content-Type: application/x-www-form-urlencoded
grant_type=authorization_code&client_id=nova&code=<code>
&redirect_uri=https%3A%2F%2Fplatform.sensenova.cn&code_verifier=<PKCE verifier>
```
- 200：
  ```json
  {"access_token":"<jwt>","expires_in":10800,"refresh_token":"<新 refresh_token>","scope":"openid offline offline_access","token_type":"bearer","id_token":"<jwt>"}
  ```
- `refresh_token` 即要写回 extra 的凭据；access_token 用于调 pool-usage。

## 与现有 refresh_token 续期闭环的关系

- 本登录链路产出的 `refresh_token` 与人工 PKCE 提取的**完全同质**——写回 `extra.refresh_token` 后，
  现有 fetcher 的 `grant_type=refresh_token` 续期 + 轮换写回逻辑继续原样工作。
- 即：登录 fallback 只在 refresh_token **缺失或明确失效**（`invalid_grant` / HTTP 401/403）时触发一次，
  成功后走既有续期查询路径。**登录也轮换**（每次刷新 rotation 语义不变）。

## 前端 JS 关键线索（如实现细节需对照）

- 登录页 chunk：`965-*.js`（is_encrypt / RSA-OAEP / EncryptJWT）、`254-*.js`（nova/login API 封装 +
  getCaptcha/checkCaptcha/smsLogin/loginNext/register）、`296-*.js`（RSA importKey）。
- API base：`{config[host].sensecoreIamApi}/iam/authn`；jwks：`{config[host].jwksBaseUrl}/.well-known/jwks.json`。
- 登录成功响应跳转：`x(n)` 取 `redirect_uri` || `redirect_to` || `redirect`。

## 安全/敏感信息处理

- 真实账号密码/refresh_token 只在调研进程内使用（临时 0600 文件），未落盘进仓库、未进任何提交。
- 实现时：Rust 需新增 `rsa` crate 做 RSA-OAEP（现有依赖已有 aes-gcm/sha1/base64/rand）；
  登录请求与密钥材料均为实现细节，测试用 mock 端点（`LLM_GATEWAY_USAGE_HTTP_OVERRIDE`）覆盖。
