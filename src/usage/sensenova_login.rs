//! SenseNova 控制台登录：账号密码 → refresh_token（纯 API）。
//!
//! 调研见 `.scratch/sensenova-login-refresh/research.md`。商汤登录是 Hydra OIDC 全流程：
//! authorize 拿 login_challenge → JWKS 取 RSA 公钥 → 密码加密成 5 段 JWE（RSA-OAEP +
//! A256GCM，auth tag 独立第 5 段）→ `nova/login` → 同 cookie 会话跟随 redirect（consent
//! 自动批准）→ `?code=` → `oauth2/token` 换 refresh_token。全程无验证码、无人工 consent。
//!
//! 本模块自建带 cookie store + 自动重定向的 reqwest client（与用量查询的 `UsageHttp`
//! 隔离，互不影响），并复用 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 把请求重定向到本地 mock
//! 供集成测试（路径保留，host 替换）。

use base64::Engine;
use reqwest::redirect::Policy;
use serde_json::Value;

use super::error::UsageError;

const PLATFORM_BASE: &str = "https://platform.sensenova.cn";
/// 商汤账号体系 REST API 独立域（research：`iam.sensecoreapi.cn/iam/authn`）。
const IAM_BASE: &str = "https://iam.sensecoreapi.cn";
const IAM_LOGIN_PATH: &str = "/iam/authn/v1/auth/nova/login";
const JWKS_URL: &str = "https://signin.sensecore.cn/.well-known/jwks.json";
const OVERRIDE_ENV: &str = "LLM_GATEWAY_USAGE_HTTP_OVERRIDE";
/// 商汤登录密码加密用的 JWK kid（Hydra id-token 签名公钥兼作 RSA-OAEP 加密公钥）。
const ENC_KEY_KID: &str = "public:hydra.openid.id-token";
const TOKEN_PATH: &str = "/oauth2/token";

/// 登录成功后的产物：新 refresh_token（写回 extra 用）。
pub struct SensenovaTokens {
    pub refresh_token: String,
}

/// 登录子客户端：带 cookie store 与自动重定向（登录链路跨域跳转多次）。
pub struct SensenovaLogin {
    client: reqwest::Client,
    base_override: Option<String>,
}

impl SensenovaLogin {
    pub fn new() -> Self {
        Self::with_proxy(None)
    }

    /// 指定 HTTP 代理创建（复用 provider 级网络代理语义）。
    pub fn with_proxy(proxy_addr: Option<&str>) -> Self {
        let mut builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(concat!("llm-gateway/", env!("CARGO_PKG_VERSION")))
            .cookie_store(true)
            .redirect(Policy::limited(8));
        if let Some(addr) = proxy_addr.map(str::trim).filter(|a| !a.is_empty())
            && let Ok(proxy) = reqwest::Proxy::all(addr)
        {
            builder = builder.proxy(proxy);
        }
        let base_override = std::env::var(OVERRIDE_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty());
        Self {
            client: builder.build().expect("reqwest client build is infallible"),
            base_override,
        }
    }

    /// 执行登录，返回新 refresh_token。任一步失败按 UsageError 语义返回（登录失败 →
    /// `Auth`；网络/上游问题按既有分类）。
    pub async fn login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<SensenovaTokens, UsageError> {
        tracing::info!(username, "SenseNova 登录开始");
        // PKCE：verifier 与 challenge 必须是同一对（challenge = S256(verifier)），
        // 换 token 时用回 verifier；此前两者各自随机导致 invalid_grant。
        let (verifier, challenge) = self.pkce_pair();
        let state = self.rand_b64u(16);
        // 1. authorize（PKCE）→ login?login_challenge=（cookie 会话开启）
        let login_challenge = self
            .start_challenge(&state, &challenge)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "登录步骤 1/6 authorize 失败");
                e
            })?;
        tracing::info!("登录步骤 1/6 authorize 成功，拿到 login_challenge");
        // 2. JWKS 取 RSA 公钥
        let pubkey = self.fetch_public_key().await.map_err(|e| {
            tracing::warn!(error = %e, "登录步骤 2/6 获取 JWKS 公钥失败");
            e
        })?;
        tracing::info!("登录步骤 2/6 JWKS 公钥获取成功");
        // 3. 密码 → 5 段 JWE
        let jwe = encrypt_password(&pubkey, password)?;
        tracing::info!("登录步骤 3/6 密码已加密为 5 段 JWE");
        // 4. nova/login → redirect（带 login_verifier）
        let redirect = self
            .nova_login(username, &jwe, &login_challenge)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "登录步骤 4/6 nova/login 失败");
                e
            })?
            .ok_or_else(|| {
                tracing::warn!("登录步骤 4/6 nova/login 成功但无 redirect（视为 Auth）");
                UsageError::Auth
            })?;
        tracing::info!("登录步骤 4/6 nova/login 成功，拿到 redirect");
        // 5. 跟随 redirect（consent 自动批准）→ ?code=（同 cookie 会话，自动重定向）
        let code = self.follow_to_code(&redirect).await.map_err(|e| {
            tracing::warn!(error = %e, "登录步骤 5/6 跟随 redirect 拿 code 失败");
            e
        })?;
        tracing::info!("登录步骤 5/6 跟随 redirect 成功，拿到 code");
        // 6. code → refresh_token（用最初的 verifier）
        let tokens = self.exchange_code(&code, &verifier).await.map_err(|e| {
            tracing::warn!(error = %e, "登录步骤 6/6 code 换 refresh_token 失败");
            e
        })?;
        tracing::info!("登录步骤 6/6 换 refresh_token 成功");
        Ok(tokens)
    }

    /// authorize 拿 login_challenge。用受限重定向跟随到登录页，再从 URL 取 challenge。
    /// `state`/`code_challenge` 由调用方生成（PKCE 配对，见 `pkce_pair`），保证换 token
    /// 时能用同一 verifier 通过校验。
    async fn start_challenge(
        &self,
        state: &str,
        code_challenge: &str,
    ) -> Result<String, UsageError> {
        let url = self.rewrite(&format!(
            "{PLATFORM_BASE}/oauth2/auth?client_id=nova&response_type=code\
             &redirect_uri={PLATFORM_BASE}&scope=openid%20offline%20offline_access\
             &state={state}&code_challenge={code_challenge}&code_challenge_method=S256",
        ));
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| UsageError::Network(format!("发起登录失败：{e}")))?;
        let final_url = resp.url().to_string();
        let challenge = parse_login_challenge(&final_url)?;
        tracing::debug!("authorize 最终 URL 含 login_challenge");
        Ok(challenge)
    }

    async fn fetch_public_key(&self) -> Result<rsa::RsaPublicKey, UsageError> {
        let url = self.rewrite(JWKS_URL);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| UsageError::Network(format!("获取登录公钥失败：{e}")))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| UsageError::Network(e.to_string()))?;
        tracing::debug!(status, "JWKS 获取返回");
        if status != 200 {
            return Err(UsageError::Upstream(status, snippet(&body)));
        }
        parse_jwks_public_key(&body)
    }

    async fn nova_login(
        &self,
        username: &str,
        jwe_password: &str,
        challenge: &str,
    ) -> Result<Option<String>, UsageError> {
        let url = self.rewrite(&format!("{IAM_BASE}{IAM_LOGIN_PATH}"));
        let body = serde_json::json!({
            "username": username,
            "password": jwe_password,
            "challenge": challenge,
            "is_encrypt": true,
        })
        .to_string();
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Origin", PLATFORM_BASE)
            .header("Referer", format!("{PLATFORM_BASE}/"))
            .body(body)
            .send()
            .await
            .map_err(|e| UsageError::Network(format!("登录请求失败：{e}")))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| UsageError::Network(e.to_string()))?;
        tracing::debug!(status, "nova/login 返回");
        if status == 401 || status == 403 {
            // 账号锁定（forbidLoginForMoment）与凭据无效分开：锁定是临时限流，
            // 再试会刷新锁定窗口，应明确提示而非当作凭据错持续重试。
            if let Some(msg) = account_lock(&text) {
                tracing::warn!(status, msg = %msg, "nova/login 账号被临时锁定");
                return Err(UsageError::Upstream(status, msg));
            }
            tracing::warn!(status, "nova/login 鉴权失败（凭据无效）");
            return Err(UsageError::Auth);
        }
        if status != 200 {
            // 400 + incorrectUsernameOrPassword → Auth；其余按上游错误。
            return if is_wrong_credentials(&text) {
                tracing::warn!(status, "nova/login 账号或密码错误");
                Err(UsageError::Auth)
            } else {
                tracing::warn!(status, body = %snippet(&text), "nova/login 上游错误");
                Err(UsageError::Upstream(status, snippet(&text)))
            };
        }
        Ok(parse_login_redirect(&text))
    }

    /// 同 cookie 会话 GET 跟随 redirect（reqwest 自动重定向 + cookie_store），返回最终 URL。
    async fn follow_to_code(&self, redirect_url: &str) -> Result<String, UsageError> {
        let resp = self
            .client
            .get(self.rewrite(redirect_url))
            .send()
            .await
            .map_err(|e| UsageError::Network(format!("跟随登录跳转失败：{e}")))?;
        let final_url = resp.url().to_string();
        tracing::debug!(final_url = %final_url, "跟随 redirect 后最终 URL");
        parse_code(&final_url).ok_or_else(|| {
            tracing::warn!(final_url = %final_url, "跟随 redirect 后 URL 无 code（视为 Auth）");
            UsageError::Auth
        })
    }

    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
    ) -> Result<SensenovaTokens, UsageError> {
        let url = self.rewrite(&format!("{PLATFORM_BASE}{TOKEN_PATH}"));
        let form = format!(
            "grant_type=authorization_code&client_id=nova&code={code}\
             &redirect_uri={PLATFORM_BASE}&code_verifier={verifier}"
        );
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|e| UsageError::Network(format!("换取 token 失败：{e}")))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| UsageError::Network(e.to_string()))?;
        tracing::debug!(status, "code 换 token 返回");
        if status != 200 {
            return Err(if status == 401 || status == 403 {
                tracing::warn!(status, "code 换 token 鉴权失败");
                UsageError::Auth
            } else {
                tracing::warn!(status, body = %snippet(&text), "code 换 token 上游错误");
                UsageError::Upstream(status, snippet(&text))
            });
        }
        let refresh_token = parse_token_response(&text).ok_or_else(|| {
            tracing::warn!("code 换 token 200 但响应无 refresh_token（视为 Auth）");
            UsageError::Auth
        })?;
        Ok(SensenovaTokens { refresh_token })
    }

    fn rand_b64u(&self, nbytes: usize) -> String {
        use rand::RngCore;
        let mut buf = vec![0u8; nbytes];
        rand::thread_rng().fill_bytes(&mut buf);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf)
    }

    /// 生成 PKCE 配对：返回 (code_verifier, code_challenge)，challenge = base64url(SHA256(verifier))。
    /// 换 token 时必须用回同一 verifier；此前 challenge 与 verifier 各自随机导致 invalid_grant。
    fn pkce_pair(&self) -> (String, String) {
        use sha2::{Digest, Sha256};
        let verifier = self.rand_b64u(32);
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        (verifier, challenge)
    }

    fn rewrite(&self, url: &str) -> String {
        match &self.base_override {
            Some(base) => {
                let path_and_query = url
                    .split_once("://")
                    .and_then(|(_, rest)| rest.find('/').map(|i| &rest[i..]))
                    .unwrap_or("/");
                format!("{}{}", base.trim_end_matches('/'), path_and_query)
            }
            None => url.to_string(),
        }
    }
}

impl Default for SensenovaLogin {
    fn default() -> Self {
        Self::new()
    }
}

// ── 纯函数：解析与判定（夹具单测） ──

/// 从最终 URL 提取 login_challenge（`/login?login_challenge=<hex>`）。
pub fn parse_login_challenge(final_url: &str) -> Result<String, UsageError> {
    let q = final_url.split('?').nth(1).unwrap_or("");
    for pair in q.split('&') {
        if let Some(v) = pair.strip_prefix("login_challenge=")
            && !v.is_empty()
        {
            return Ok(v.to_string());
        }
    }
    Err(UsageError::Auth)
}

/// 登录失败判定：400 响应体是商汤 gRPC 风格错误且 reason=incorrectUsernameOrPassword。
fn is_wrong_credentials(body: &str) -> bool {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    v.get("details")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter().any(|d| {
                d.get("reason").and_then(Value::as_str) == Some("incorrectUsernameOrPassword")
            })
        })
        .unwrap_or(false)
}

/// 商汤账号被临时锁定（连错多次触发，`forbidLoginForMoment`）。此时再试只会
/// 刷新锁定窗口，应区别于「凭据无效」并避免反复撞锁。
fn account_lock(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let locked = v
        .get("details")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .any(|d| d.get("reason").and_then(Value::as_str) == Some("forbidLoginForMoment"))
        })
        .unwrap_or(false);
    if !locked {
        return None;
    }
    // 优先取 LocalizedMessage 里的人类可读提示（如「账号已被锁定，请 10 分钟后再试」）。
    let msg = v
        .get("details")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter().find_map(|d| {
                d.get("message")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
        })
        .unwrap_or("账号已被临时锁定");
    Some(msg.to_string())
}

/// 登录成功响应取 redirect（优先 redirect，其次 redirect_uri，无则空 → None）。
fn parse_login_redirect(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    ["redirect", "redirect_uri"]
        .iter()
        .filter_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

/// 从跟随后的最终 URL 提取授权码 `code`。
fn parse_code(final_url: &str) -> Option<String> {
    let q = final_url.split('?').nth(1).unwrap_or("");
    q.split('&')
        .find_map(|pair| pair.strip_prefix("code="))
        .map(|c| c.to_string())
        .filter(|c| !c.is_empty())
}

/// 解析 oauth2/token 成功响应中的 refresh_token。
fn parse_token_response(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 从 JWKS JSON 取指定 kid 的 RSA 公钥。
fn parse_jwks_public_key(body: &str) -> Result<rsa::RsaPublicKey, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("JWKS 不是合法 JSON：{e}")))?;
    let key = v
        .get("keys")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|k| k.get("kid").and_then(Value::as_str) == Some(ENC_KEY_KID))
        })
        .ok_or(UsageError::Auth)?;
    let n_b64 = key
        .get("n")
        .and_then(Value::as_str)
        .ok_or(UsageError::Auth)?;
    let e_b64 = key
        .get("e")
        .and_then(Value::as_str)
        .ok_or(UsageError::Auth)?;
    let n_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(n_b64)
        .map_err(|_| UsageError::Auth)?;
    let e_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(e_b64)
        .map_err(|_| UsageError::Auth)?;
    let n = rsa::BigUint::from_bytes_be(&n_bytes);
    let e = rsa::BigUint::from_bytes_be(&e_bytes);
    rsa::RsaPublicKey::new(n, e).map_err(|_| UsageError::Auth)
}

/// 把密码加密成商汤登录接受的 5 段 JWE：header.encryptedKey.iv.ciphertext.tag。
///
/// 注意：标准 JWE compact 是 4 段（ciphertext 尾部带 16 字节 tag）；商汤服务端解析器
/// 期望 tag 独立成第 5 段（实测 4 段会被判密码错误）。加密参数 RSA-OAEP(SHA-1) 包裹
/// 32 字节 CEK + AES-256-GCM 加密明文。
pub fn encrypt_password(
    public_key: &rsa::RsaPublicKey,
    password: &str,
) -> Result<String, UsageError> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    use rand::RngCore;
    use rsa::Oaep;

    let header = r#"{"alg":"RSA-OAEP","enc":"A256GCM"}"#;
    // JWE 标准：AES-GCM 的附加认证数据（AAD）= base64url(header)，同时作为产物第 1 段。
    let aad = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header.as_bytes());

    // 32 字节 CEK（AES-256）+ 12 字节 IV。
    let mut cek = [0u8; 32];
    let mut iv = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut cek);
    rand::thread_rng().fill_bytes(&mut iv);

    // AES-256-GCM：密文（不含 tag）与 16 字节 tag。AAD 必须参与认证——
    // 商汤服务端按 JWE 标准用 base64url(header) 做 AAD 解包，缺失会判「密码错误」。
    let cipher = Aes256Gcm::new_from_slice(&cek).map_err(|_| UsageError::Auth)?;
    let ct_and_tag = cipher
        .encrypt(
            &Nonce::from(iv),
            Payload {
                msg: password.as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| UsageError::Auth)?;
    let (ct, tag) = ct_and_tag.split_at(ct_and_tag.len() - 16);

    // RSA-OAEP(SHA-1) 包裹 CEK。
    let encrypted_key = public_key
        .encrypt(&mut rand::thread_rng(), Oaep::new::<sha1::Sha1>(), &cek)
        .map_err(|_| UsageError::Auth)?;

    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    Ok(format!(
        "{}.{}.{}.{}.{}",
        aad,
        b64(&encrypted_key),
        b64(&iv),
        b64(ct),
        b64(tag)
    ))
}

/// 错误消息中的响应体片段（截断 200 字符）。
fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() > 200 {
        format!("{}…", trimmed.chars().take(200).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::traits::PublicKeyParts;

    #[test]
    fn pkce_pair_challenge_is_s256_of_verifier() {
        let login = SensenovaLogin::new();
        let (verifier, challenge) = login.pkce_pair();
        assert!(!verifier.is_empty());
        // challenge 必须等于 base64url(SHA256(verifier))，否则换 token 时 invalid_grant。
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let expect = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expect);
        // 两次生成不同（随机性）。
        let (v2, c2) = login.pkce_pair();
        assert_ne!((verifier, challenge), (v2, c2));
    }

    #[test]
    fn parse_login_challenge_extracts_hex() {
        let url = "https://platform.sensenova.cn/login?login_challenge=0c2af45fded2415db7402d2d02de53d7&x=1";
        assert_eq!(
            parse_login_challenge(url).unwrap(),
            "0c2af45fded2415db7402d2d02de53d7"
        );
    }

    #[test]
    fn parse_login_challenge_missing_is_auth() {
        assert!(matches!(
            parse_login_challenge("https://platform.sensenova.cn/login"),
            Err(UsageError::Auth)
        ));
    }

    #[test]
    fn wrong_credentials_detected_from_grpc_error_body() {
        let body = r#"{"code":3,"message":"InvalidArgument","details":[
            {"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"incorrectUsernameOrPassword","domain":"iam","metadata":{}},
            {"@type":"type.googleapis.com/google.rpc.LocalizedMessage","locale":"zh-CN","message":"账号或密码错误"}
        ]}"#;
        assert!(is_wrong_credentials(body));
        // 非凭据错误 / 非 JSON → false。
        let captcha = r#"{"code":3,"details":[{"reason":"captcha_required"}]}"#;
        assert!(!is_wrong_credentials(captcha));
        assert!(!is_wrong_credentials("not json"));
        assert!(!is_wrong_credentials("{}"));
    }

    #[test]
    fn account_lock_detected_with_message() {
        // forbidLoginForMoment（连续失败触发临时锁定）应识别并取可读提示。
        let body = r#"{"code":7,"message":"PermissionDenied","details":[
            {"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"forbidLoginForMoment","domain":"iam","metadata":{}},
            {"@type":"type.googleapis.com/google.rpc.LocalizedMessage","locale":"zh-CN","message":"账号已锁定，请 10 分钟后再试"}
        ]}"#;
        let msg = account_lock(body).expect("应识别账号锁定");
        assert!(msg.contains("10 分钟"), "应带可读提示：{msg}");
        // 凭据错误不是锁定。
        let wrong = r#"{"code":3,"details":[{"reason":"incorrectUsernameOrPassword"}]}"#;
        assert!(account_lock(wrong).is_none());
        assert!(account_lock("not json").is_none());
    }

    #[test]
    fn parse_login_redirect_prefers_redirect_field() {
        let body = r#"{"redirect":"https://platform.sensenova.cn/oauth2/auth?x=1","redirect_uri":"https://other","redirect_to":""}"#;
        assert_eq!(
            parse_login_redirect(body).unwrap(),
            "https://platform.sensenova.cn/oauth2/auth?x=1"
        );
        // 只有 redirect_uri 时回退到它。
        let only_uri = r#"{"redirect":"","redirect_uri":"https://fallback"}"#;
        assert_eq!(parse_login_redirect(only_uri).unwrap(), "https://fallback");
        // 全空 → None。
        let empty = r#"{"redirect":"","redirect_uri":""}"#;
        assert!(parse_login_redirect(empty).is_none());
        assert!(parse_login_redirect("not json").is_none());
    }

    #[test]
    fn parse_code_from_final_url() {
        let url = "https://platform.sensenova.cn/?code=AcG7_bWXvZGmK0zE9rxlV7vNi4ShveZWbf83TQkoCfQ.hcgsr5Kdsh5S7PYec2RNbA3w0Q2P3SSgSFd9FZ4Dy6s&scope=openid+offline+offline_access&state=abc";
        assert_eq!(
            parse_code(url).unwrap(),
            "AcG7_bWXvZGmK0zE9rxlV7vNi4ShveZWbf83TQkoCfQ.hcgsr5Kdsh5S7PYec2RNbA3w0Q2P3SSgSFd9FZ4Dy6s"
        );
        assert!(parse_code("https://platform.sensenova.cn/login").is_none());
    }

    #[test]
    fn parse_token_response_extracts_refresh_token() {
        let body = r#"{"access_token":"at","expires_in":10800,"refresh_token":"rt-new","scope":"openid offline offline_access","token_type":"bearer"}"#;
        assert_eq!(parse_token_response(body).unwrap(), "rt-new");
        // 无 refresh_token（如仅 access_token 的错误响应）→ None。
        let no_rt = r#"{"access_token":"at","expires_in":10800}"#;
        assert!(parse_token_response(no_rt).is_none());
    }

    #[test]
    fn jwks_public_key_parse_and_roundtrip() {
        // 用真实 JWKS 结构：本测试生成一个临时 RSA 密钥的 JWK 并断言能解析出公钥。
        // 固定使用已知的 JWKS JSON（来自 signin.sensecore.cn 结构）。
        let body = r#"{"keys":[
            {"kid":"public:hydra.openid.id-token","kty":"RSA","alg":"RS256","use":"sig",
             "n":"5nsU994-8lnsOb93Lzu8lIYr92Rhdyw7UXaEKBpIRJYdVQRKFUFynWUS-MlDi19STFK_PvYBmC0fTLhfsTEp-zJIPuBLhpvW_3nHwtiLnlhCuRTelZYwsIsMds2-4gCx_bynVKSp6ZvdZ7781mWvy3zpVuG-2z02YSno1Yi_txVTjXzZnb0Jf_EOjbWjh9N6s-gaTVLVu34gZ0vkEICQ_Mn1mzdMVpcBfN4v7KxnsiyjYorGAdeMwPxAyPlIFi1oxKhknLZTWGuypURZp2adMY9CiK0yZqVR3TaRgQ3cowrTHW-oIbXq5lHFVNickn_NnBq-wiGgwjgsg54lFDvWrw",
             "e":"AQAB"},
            {"kid":"other","kty":"RSA","n":"AAEAAQ","e":"AQAB"}
        ]}"#;
        let key = parse_jwks_public_key(body).unwrap();
        assert_eq!(key.size(), 256); // 2048-bit = 256 bytes
        // 缺目标 kid → Auth。
        let missing = r#"{"keys":[{"kid":"other","n":"AAEAAQ","e":"AQAB"}]}"#;
        assert!(matches!(
            parse_jwks_public_key(missing),
            Err(UsageError::Auth)
        ));
    }

    #[test]
    fn encrypt_password_produces_5_segment_jwe() {
        // 生成临时 RSA-2048 密钥对，加密后解包验证结构（header/key/iv/ct/tag 五段）。
        let priv_key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let pub_key = rsa::RsaPublicKey::from(&priv_key);
        let jwe = encrypt_password(&pub_key, "hunter2pass").unwrap();
        let segs: Vec<&str> = jwe.split('.').collect();
        assert_eq!(segs.len(), 5, "商汤要求 5 段 JWE（tag 独立）");
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(segs[0])
                .unwrap(),
            br#"{"alg":"RSA-OAEP","enc":"A256GCM"}"#
        );

        // 完整解包验证可还原密码（用私钥解 CEK → AES-GCM 解密）。
        // JWE 标准：AES-GCM 的 AAD = base64url(header)（即第 1 段），解密必须带它。
        use aes_gcm::aead::{Aead, KeyInit, Payload};
        use aes_gcm::{Aes256Gcm, Nonce};
        use rsa::Oaep;
        let enc_key = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segs[1])
            .unwrap();
        let iv = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segs[2])
            .unwrap();
        let ct = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segs[3])
            .unwrap();
        let tag = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segs[4])
            .unwrap();
        let cek = priv_key
            .decrypt(Oaep::new::<sha1::Sha1>(), &enc_key)
            .unwrap();
        let mut ct_tag = ct.clone();
        ct_tag.extend_from_slice(&tag);
        let cipher = Aes256Gcm::new_from_slice(&cek).unwrap();
        let mut iv_arr = [0u8; 12];
        iv_arr.copy_from_slice(&iv);
        let aad = segs[0].as_bytes();
        // 带 AAD 解密成功（服务端解包路径）。
        let plain = cipher
            .decrypt(&Nonce::from(iv_arr), Payload { msg: &ct_tag, aad })
            .unwrap();
        assert_eq!(plain, b"hunter2pass");
        // 不带 AAD 解密必须失败——证明 AAD 参与认证（缺失即「密码错误」的根因）。
        assert!(
            cipher
                .decrypt(&Nonce::from(iv_arr), ct_tag.as_slice())
                .is_err(),
            "缺 AAD 应无法解密（商汤服务端解包会因此判密码错）"
        );
    }
}
