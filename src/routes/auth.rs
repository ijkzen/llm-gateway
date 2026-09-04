use std::sync::OnceLock;

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};

use crate::auth::{
    self, AuthedUser, SESSION_COOKIE, clear_session_cookie, create_session,
    delete_expired_sessions, hash_password, revoke_other_sessions, revoke_session, session_user,
    verify_password,
};
use crate::entity::user::{self, ActiveModel, Entity};
use crate::i18n::Lang;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/init", post(init))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/change-password", post(change_password))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    /// 是否已有用户（false 表示需要走初始化流程）。
    initialized: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserResponse {
    username: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

/// 用户名/密码基础校验，返回错误消息。
fn validate_credentials(username: &str, password: &str, lang: Lang) -> Option<String> {
    if username.is_empty() || username.len() > 64 {
        return Some(
            lang.tr("用户名需为 1-64 个字符", "username must be 1-64 characters")
                .to_string(),
        );
    }
    if password.len() < 6 || password.len() > 128 {
        return Some(
            lang.tr(
                "密码长度需为 6-128 个字符",
                "password must be 6-128 characters",
            )
            .to_string(),
        );
    }
    None
}

/// GET /api/auth/status：是否已完成初始化。
async fn status(State(state): State<AppState>) -> AxumResponse {
    match Entity::find().count(&state.db).await {
        Ok(count) => (
            StatusCode::OK,
            Json(Response::success(StatusResponse {
                initialized: count > 0,
            })),
        )
            .into_response(),
        Err(e) => response::db_error::<()>(e.to_string()).into_response(),
    }
}

/// POST /api/auth/init：仅当用户表为空时创建首个用户，并直接建立会话。
async fn init(State(state): State<AppState>, Json(req): Json<CredentialsRequest>) -> AxumResponse {
    let lang = state.settings.lang().await;
    let username = req.username.trim();
    if let Some(msg) = validate_credentials(username, &req.password, lang) {
        return response::bad_request::<()>(msg).into_response();
    }

    match Entity::find().count(&state.db).await {
        Ok(count) if count > 0 => {
            let msg = lang.tr(
                "系统已初始化，请直接登录",
                "system is already initialized, please log in",
            );
            return response::bad_request::<()>(msg).into_response();
        }
        Ok(_) => {}
        Err(e) => return response::db_error::<()>(e.to_string()).into_response(),
    }

    let password_hash = match hash_password(&req.password) {
        Ok(hash) => hash,
        Err(e) => return response::internal_error::<()>(e.to_string()).into_response(),
    };

    let now = chrono::Utc::now();
    let active = ActiveModel {
        username: Set(username.to_string()),
        password_hash: Set(password_hash),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    match active.insert(&state.db).await {
        Ok(model) => match create_session(&state.db, model.id).await {
            Ok((token, expires_at)) => login_response(&model.username, &token, expires_at),
            Err(e) => response::internal_error::<()>(e.to_string()).into_response(),
        },
        Err(e) if crate::db::is_unique_violation(&e) => response::bad_request::<()>(
            lang.tr("同名用户已存在", "a user with the same name already exists"),
        )
        .into_response(),
        Err(e) => response::db_error::<()>(e.to_string()).into_response(),
    }
}

/// POST /api/auth/login：校验用户名密码，建立会话。
async fn login(State(state): State<AppState>, Json(req): Json<CredentialsRequest>) -> AxumResponse {
    let lang = state.settings.lang().await;
    delete_expired_sessions(&state.db).await;

    let username = req.username.trim();
    let model = match Entity::find()
        .filter(user::Column::Username.eq(username))
        .one(&state.db)
        .await
    {
        Ok(model) => model,
        Err(e) => return response::db_error::<()>(e.to_string()).into_response(),
    };

    // 用户不存在时也对固定密码做一次等价校验，避免通过响应时间探测用户名。
    static DUMMY_HASH: OnceLock<String> = OnceLock::new();
    let dummy =
        DUMMY_HASH.get_or_init(|| hash_password("dummy-password-for-timing").unwrap_or_default());
    let verified = match &model {
        Some(model) => verify_password(&req.password, &model.password_hash),
        None => {
            let _ = verify_password(&req.password, dummy);
            false
        }
    };

    let Some(model) = model.filter(|_| verified) else {
        let msg = lang.tr("用户名或密码错误", "invalid username or password");
        return response::unauthorized::<()>(msg).into_response();
    };

    match create_session(&state.db, model.id).await {
        Ok((token, expires_at)) => login_response(&model.username, &token, expires_at),
        Err(e) => response::internal_error::<()>(e.to_string()).into_response(),
    }
}

/// 统一的登录/初始化成功响应：JSON body + Set-Cookie。
fn login_response(
    username: &str,
    token: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> AxumResponse {
    let max_age = (expires_at - chrono::Utc::now())
        .num_seconds()
        .clamp(0, 7 * 24 * 3600);
    let cookie =
        format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}");
    (
        [(header::SET_COOKIE, cookie)],
        Json(Response::success(UserResponse {
            username: username.to_string(),
        })),
    )
        .into_response()
}

/// POST /api/auth/logout：吊销当前会话并清除 Cookie。
async fn logout(State(state): State<AppState>, headers: axum::http::HeaderMap) -> AxumResponse {
    if let Some(token) = auth::extract_cookie(&headers, SESSION_COOKIE) {
        revoke_session(&state.db, &token).await;
    }
    (
        [(header::SET_COOKIE, clear_session_cookie())],
        Json(Response::success(())),
    )
        .into_response()
}

/// GET /api/auth/me：当前登录用户。
async fn me(Extension(user): Extension<AuthedUser>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(Response::success(UserResponse {
            username: user.username,
        })),
    )
}

/// POST /api/auth/change-password：校验旧密码，更新后吊销其他会话（保留当前）。
async fn change_password(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> AxumResponse {
    let lang = state.settings.lang().await;
    let session_msg = lang.tr("未登录或登录已过期", "not logged in or session expired");
    let Some(token) = auth::extract_cookie(&headers, SESSION_COOKIE) else {
        return response::unauthorized::<()>(session_msg).into_response();
    };
    let Some(user) = (match session_user(&state.db, &token).await {
        Ok(user) => user,
        Err(e) => return response::internal_error::<()>(e.to_string()).into_response(),
    }) else {
        return response::unauthorized::<()>(session_msg).into_response();
    };

    if !verify_password(&req.old_password, &user.password_hash) {
        return response::bad_request::<()>(lang.tr("旧密码不正确", "old password is incorrect"))
            .into_response();
    }
    if let Some(msg) = validate_credentials(&user.username, &req.new_password, lang) {
        return response::bad_request::<()>(msg).into_response();
    }

    let password_hash = match hash_password(&req.new_password) {
        Ok(hash) => hash,
        Err(e) => return response::internal_error::<()>(e.to_string()).into_response(),
    };

    let user_id = user.id;
    let mut active: ActiveModel = user.into();
    active.password_hash = Set(password_hash);
    active.updated_at = Set(chrono::Utc::now());

    if let Err(e) = active.update(&state.db).await {
        return response::db_error::<()>(e.to_string()).into_response();
    }
    revoke_other_sessions(&state.db, user_id, &token).await;

    (StatusCode::OK, Json(Response::success(()))).into_response()
}
