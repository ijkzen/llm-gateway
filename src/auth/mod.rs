//! 登录认证：密码哈希、会话管理、API Key 鉴权与请求拦截中间件。
//!
//! 管理后台（`/api/*`）使用 Cookie Session（HttpOnly，服务端 session 表）；
//! `/v1/*` 使用 `Authorization: Bearer` 校验 api_key 表。

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response as AxumResponse},
};
use chrono::{DateTime, Utc};
use rand::RngCore;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::entity::{api_key, session, user};
use crate::i18n::Lang;
use crate::response::Response;
use crate::state::AppState;

/// 会话 Cookie 名称。
pub const SESSION_COOKIE: &str = "lg_session";
/// 会话有效期：7 天。
pub const SESSION_TTL_SECS: i64 = 7 * 24 * 3600;

/// 已通过会话认证的管理用户（注入 /api 请求 extensions）。
#[derive(Clone, Debug)]
pub struct AuthedUser {
    pub username: String,
}

/// 已通过 Bearer 认证的调用方 API Key（注入 /v1 请求 extensions）。
#[derive(Clone, Debug)]
pub struct AuthedApiKey {
    pub id: i32,
    pub name: String,
}

// ---------- 密码哈希（argon2id） ----------

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| anyhow::anyhow!("failed to hash password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

// ---------- 会话 ----------

/// 生成 256 位随机 hex 会话令牌（Cookie 值；库中只存其 SHA-256）。
pub fn new_session_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 令牌摘要（session 表主键）。
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 创建会话，返回 (明文令牌, 过期时间)。
pub async fn create_session(
    db: &DatabaseConnection,
    user_id: i32,
) -> anyhow::Result<(String, DateTime<Utc>)> {
    let token = new_session_token();
    let now = Utc::now();
    let expires_at = now + chrono::Duration::seconds(SESSION_TTL_SECS);
    let active = session::ActiveModel {
        id: Set(hash_token(&token)),
        user_id: Set(user_id),
        created_at: Set(now),
        expires_at: Set(expires_at),
    };
    active.insert(db).await?;
    Ok((token, expires_at))
}

/// 校验会话令牌，返回归属用户。过期会话顺带删除并视为无效。
pub async fn session_user(
    db: &DatabaseConnection,
    token: &str,
) -> anyhow::Result<Option<user::Model>> {
    let now = Utc::now();
    let Some(session) = session::Entity::find_by_id(hash_token(token))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    if session.expires_at <= now {
        session::Entity::delete_by_id(session.id.clone())
            .exec(db)
            .await?;
        return Ok(None);
    }
    Ok(user::Entity::find_by_id(session.user_id).one(db).await?)
}

/// 清理全部过期会话（登录时调用）。
pub async fn delete_expired_sessions(db: &DatabaseConnection) {
    let _ = session::Entity::delete_many()
        .filter(session::Column::ExpiresAt.lte(Utc::now()))
        .exec(db)
        .await;
}

/// 吊销单个会话（登出）。
pub async fn revoke_session(db: &DatabaseConnection, token: &str) {
    let _ = session::Entity::delete_by_id(hash_token(token))
        .exec(db)
        .await;
}

/// 吊销指定用户的其他会话（修改密码后踢掉旧登录，保留当前会话）。
pub async fn revoke_other_sessions(db: &DatabaseConnection, user_id: i32, keep_token: &str) {
    let keep_id = hash_token(keep_token);
    let _ = session::Entity::delete_many()
        .filter(session::Column::UserId.eq(user_id))
        .filter(session::Column::Id.ne(keep_id))
        .exec(db)
        .await;
}

// ---------- Cookie ----------

/// 从 Cookie 头中提取指定名称的值。
pub fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=')
            && key.trim() == name
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// 会话 Cookie 的 Set-Cookie 值。
pub fn session_cookie(token: &str) -> String {
    format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_TTL_SECS}")
}

/// 清除会话 Cookie 的 Set-Cookie 值。
pub fn clear_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

// ---------- /v1 Bearer 鉴权 ----------

/// 当前进程语言（中间件等无 State 场景用；默认 zh-CN）。
async fn current_lang() -> Lang {
    match crate::app_settings::AppSettings::process_global() {
        Some(settings) => settings.lang().await,
        None => Lang::default(),
    }
}

/// 校验 Bearer API Key。命中启用的 key 返回其信息，否则返回 401 信封错误。
pub async fn authorize_api_key(
    db: &DatabaseConnection,
    headers: &HeaderMap,
) -> Result<AuthedApiKey, Response<()>> {
    let Some(token) = extract_bearer(headers) else {
        return Err(unauthorized_api_key().await);
    };
    let key_hash = hash_token(&token);
    match api_key::Entity::find()
        .filter(api_key::Column::KeyHash.eq(key_hash))
        .filter(api_key::Column::Enable.eq(true))
        .one(db)
        .await
    {
        Ok(Some(model)) => Ok(AuthedApiKey {
            id: model.id,
            name: model.name,
        }),
        Ok(None) => Err(unauthorized_api_key().await),
        Err(e) => {
            let lang = current_lang().await;
            let msg = if lang == Lang::En {
                format!("API key validation failed: {e}")
            } else {
                format!("API Key 校验失败：{e}")
            };
            Err(Response::<()>::error(crate::response::INTERNAL_ERROR, msg))
        }
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

async fn unauthorized_api_key() -> Response<()> {
    let lang = current_lang().await;
    let msg = lang.tr("无效的 API Key", "invalid API Key");
    Response::error("INVALID_API_KEY", msg)
}

// ---------- 中间件 ----------

/// 请求拦截：
/// - `/api/*`（除 `/api/auth/status|login|init`、`/api/healthz`）要求有效会话 Cookie，
///   认证后的用户信息注入 extensions（`AuthedUser`）；
/// - `/v1/*` 要求有效 Bearer API Key，key 信息注入 extensions（`AuthedApiKey`）；
/// - 其余路径（SPA 静态资源）直接放行。
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> AxumResponse {
    let path = req.uri().path();

    let auth_public = path == "/api/healthz"
        || path == "/api/auth/status"
        || path == "/api/auth/login"
        || path == "/api/auth/init";
    if auth_public {
        return next.run(req).await;
    }
    if !path.starts_with("/api/") && !path.starts_with("/v1/") {
        return next.run(req).await;
    }

    if path.starts_with("/v1/") {
        return match authorize_api_key(&state.db, req.headers()).await {
            Ok(key) => {
                let mut req = req;
                req.extensions_mut().insert(key);
                next.run(req).await
            }
            Err(body) => {
                let status = if body.error_code == crate::response::INTERNAL_ERROR {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    StatusCode::UNAUTHORIZED
                };
                openai_error_response(status, &body.error_message, &body.error_code)
            }
        };
    }

    let Some(token) = extract_cookie(req.headers(), SESSION_COOKIE) else {
        return unauthorized_session().await;
    };
    match session_user(&state.db, &token).await {
        Ok(Some(user)) => {
            let mut req = req;
            req.extensions_mut().insert(AuthedUser {
                username: user.username,
            });
            next.run(req).await
        }
        Ok(None) => unauthorized_session().await,
        Err(e) => {
            let lang = current_lang().await;
            let msg = if lang == Lang::En {
                format!("session validation failed: {e}")
            } else {
                format!("会话校验失败：{e}")
            };
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(Response::<()>::error(crate::response::INTERNAL_ERROR, msg)),
            )
                .into_response()
        }
    }
}

async fn unauthorized_session() -> AxumResponse {
    let lang = current_lang().await;
    let msg = lang.tr("未登录或登录已过期", "not logged in or session expired");
    (
        StatusCode::UNAUTHORIZED,
        Json(Response::<()>::error(crate::response::UNAUTHORIZED, msg)),
    )
        .into_response()
}

/// OpenAI 格式错误（/v1 鉴权失败等场景）。
fn openai_error_response(status: StatusCode, message: &str, code: &str) -> AxumResponse {
    (
        status,
        Json(json!({ "error": { "message": message, "type": "invalid_request_error", "code": code } })),
    )
        .into_response()
}

/// OpenAI 格式错误响应，供 /v1 路由复用。
pub fn openai_error(
    status: StatusCode,
    message: impl Into<String>,
    error_type: &str,
    code: &str,
) -> AxumResponse {
    (
        status,
        Json(json!({ "error": { "message": message.into(), "type": error_type, "code": code } })),
    )
        .into_response()
}

/// 启动时回填 api_key.key_hash（migration 7 新增列；历史数据无法在 SQL 内解密计算）。
pub async fn backfill_api_key_hashes(db: &DatabaseConnection) {
    let rows = match api_key::Entity::find()
        .filter(api_key::Column::KeyHash.is_null())
        .all(db)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("Failed to load api keys for key_hash backfill: {e}");
            return;
        }
    };
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|m| m.key_hash.as_deref().unwrap_or("").is_empty())
        .collect();
    if rows.is_empty() {
        return;
    }
    let mut updated = 0;
    for model in rows {
        let plain = match crate::crypto::decrypt(&model.key) {
            Ok(plain) if !plain.is_empty() => plain,
            _ => {
                tracing::warn!(
                    "api_key '{}' 无法解密，跳过 key_hash 回填（该 key 将无法用于 /v1 鉴权）",
                    model.name
                );
                continue;
            }
        };
        let mut active: api_key::ActiveModel = model.into();
        active.key_hash = Set(Some(hash_token(&plain)));
        if active.update(db).await.is_ok() {
            updated += 1;
        }
    }
    if updated > 0 {
        tracing::info!("Backfilled key_hash for {updated} api key(s)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let hash = hash_password("Password").unwrap();
        assert_ne!(hash, "Password");
        assert!(verify_password("Password", &hash));
        assert!(!verify_password("password", &hash));
        assert!(!verify_password("Password", "not-a-hash"));
    }

    #[test]
    fn token_hash_is_deterministic_sha256_hex() {
        let a = hash_token("abc");
        let b = hash_token("abc");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(a, hash_token("abd"));
    }

    #[test]
    fn session_token_is_random_hex() {
        let a = new_session_token();
        let b = new_session_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn extract_cookie_parses_pairs() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "a=1; lg_session=tok ; b=2".parse().unwrap(),
        );
        assert_eq!(
            extract_cookie(&headers, SESSION_COOKIE).as_deref(),
            Some("tok")
        );
        assert_eq!(extract_cookie(&headers, "missing"), None);
    }

    #[test]
    fn cookie_values_roundtrip() {
        let set = session_cookie("tok");
        assert!(set.starts_with("lg_session=tok;"));
        assert!(set.contains("HttpOnly"));
        assert!(set.contains("SameSite=Lax"));
        let clear = clear_session_cookie();
        assert!(clear.contains("Max-Age=0"));
    }

    #[test]
    fn extract_bearer_supports_case_insensitive_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer lg-abc".parse().unwrap(),
        );
        assert_eq!(extract_bearer(&headers).as_deref(), Some("lg-abc"));
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "bearer lg-abc".parse().unwrap(),
        );
        assert_eq!(extract_bearer(&headers).as_deref(), Some("lg-abc"));
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Token lg-abc".parse().unwrap(),
        );
        assert_eq!(extract_bearer(&headers), None);
    }
}
