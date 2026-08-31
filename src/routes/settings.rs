use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde::{Deserialize, Serialize};

use crate::app_settings::{KEY_LANGUAGE, KEY_TIMEZONE};
use crate::entity::setting;
use crate::i18n::Lang;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_settings))
        .route("/{key}", put(update_setting))
}

#[derive(Serialize)]
struct SettingResponse {
    key: String,
    value: String,
    r#type: String,
    updated_at: String,
}

impl From<setting::Model> for SettingResponse {
    fn from(model: setting::Model) -> Self {
        let type_str = match setting::SettingType::try_from(model.r#type) {
            Ok(t) => t.to_string(),
            Err(_) => "Unknown".to_string(),
        };
        Self {
            key: model.key,
            value: model.value,
            r#type: type_str,
            updated_at: model.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Deserialize)]
struct UpdateSettingRequest {
    value: String,
}

/// Validates `value` against the setting's declared type. String (and
/// unknown legacy type values) accept anything; `language` and `timezone`
/// get extra value-level validation on top.
fn validate_setting_value(
    setting_type: i32,
    key: &str,
    value: &str,
    lang: Lang,
) -> Result<(), String> {
    match setting::SettingType::try_from(setting_type) {
        Ok(setting::SettingType::Int) => {
            if value.trim().parse::<i64>().is_err() {
                return Err(lang
                    .tr("value 必须是有效的整数", "value must be a valid integer")
                    .to_string());
            }
        }
        Ok(setting::SettingType::Float) => {
            if value.trim().parse::<f64>().is_err() {
                return Err(lang
                    .tr("value 必须是有效的数字", "value must be a valid number")
                    .to_string());
            }
        }
        Ok(setting::SettingType::Bool) if !matches!(value, "true" | "false") => {
            return Err(lang
                .tr("value 必须是 true 或 false", "value must be true or false")
                .to_string());
        }
        _ => {}
    }

    if key == KEY_LANGUAGE && value.trim().parse::<Lang>().is_err() {
        return Err(lang
            .tr(
                "language 仅支持 zh-CN 或 en",
                "language must be zh-CN or en",
            )
            .to_string());
    }
    if key == KEY_TIMEZONE && value.trim().parse::<chrono_tz::Tz>().is_err() {
        return Err(lang
            .tr(
                "timezone 必须是合法的 IANA 时区（如 Asia/Shanghai）",
                "timezone must be a valid IANA timezone (e.g. Asia/Shanghai)",
            )
            .to_string());
    }
    Ok(())
}

async fn list_settings(State(state): State<AppState>) -> impl IntoResponse {
    match setting::Entity::find().all(&state.db).await {
        Ok(settings) => {
            let response: Vec<SettingResponse> = settings.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn update_setting(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<UpdateSettingRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    match setting::Entity::find_by_id(&key).one(&state.db).await {
        Ok(Some(model)) => {
            if let Err(msg) = validate_setting_value(model.r#type, &key, &req.value, lang) {
                return response::bad_request(msg);
            }

            // 时区变更需要重建全部定时任务（cron 语义时区切换）：
            // 先在内存里用新时区重建并重算 next_run_at，再落库。
            let timezone_changed = key == KEY_TIMEZONE && model.value != req.value;
            if timezone_changed {
                let repo = crate::cron::repository::SeaOrmCronJobRepository::new(state.db.clone());
                if let Err(e) = state.scheduler.reload_all_jobs(&repo).await {
                    tracing::error!("Failed to reload cron jobs after timezone change: {}", e);
                    return response::scheduler_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        lang.tr(
                            "时区变更后重建定时任务失败",
                            "failed to rebuild cron jobs after timezone change",
                        ),
                    );
                }
            }

            let mut active: setting::ActiveModel = model.into();
            active.value = Set(req.value.clone());
            active.updated_at = Set(chrono::Utc::now());

            match active.update(&state.db).await {
                Ok(_) => {
                    // 落库成功后刷新进程内缓存（失败时数据库已是新值，
                    // 缓存以数据库为准，不阻塞响应）。
                    state.settings.update(&key, &req.value).await;

                    // 语言切换：把未自定义标题/描述的任务同步为目标语言默认文案。
                    if key == KEY_LANGUAGE {
                        let new_lang = match req.value.parse::<Lang>() {
                            Ok(l) => l,
                            Err(_) => lang,
                        };
                        let repo =
                            crate::cron::repository::SeaOrmCronJobRepository::new(state.db.clone());
                        match state
                            .scheduler
                            .sync_titles_to_language(&repo, new_lang)
                            .await
                        {
                            Ok(n) if n > 0 => {
                                tracing::info!(
                                    "Synced {n} cron job title(s)/description(s) after language change"
                                );
                            }
                            Ok(_) => {}
                            Err(e) => tracing::error!(
                                "Failed to sync cron job titles after language change: {}",
                                e
                            ),
                        }
                    }

                    (StatusCode::OK, Json(Response::success(())))
                }
                Err(e) => response::db_error(e.to_string()),
            }
        }
        Ok(None) => {
            let msg = if lang == Lang::En {
                format!("setting '{key}' does not exist")
            } else {
                format!("设置 '{key}' 不存在")
            };
            response::not_found(msg)
        }
        Err(e) => response::db_error(e.to_string()),
    }
}
