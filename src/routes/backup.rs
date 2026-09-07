use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};

use crate::backup::{self, BackupFile};
use crate::i18n::Lang;
use crate::response::{self, Response};
use crate::routes::providers::{
    validate_extra, validate_json_field, validate_protocol_billing, validate_proxy,
};
use crate::routes::settings::validate_setting_value;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/export", get(export_backup))
        .route("/import", post(import_backup))
}

/// GET /api/backup/export：全量配置备份（含明文密钥），由前端触发下载。
async fn export_backup(State(state): State<AppState>) -> impl IntoResponse {
    match backup::build_export(&state.db).await {
        Ok(file) => (StatusCode::OK, Json(Response::success(file))),
        Err(e) => response::db_error(e.to_string()),
    }
}

/// POST /api/backup/import：整体替换导入。body 为备份 JSON 原文。
/// 格式/校验/应用任一步失败都返回 400 + 具体中文错误（前端错误弹窗展示）。
async fn import_backup(State(state): State<AppState>, body: String) -> impl IntoResponse {
    let file = match backup::parse_backup(&body) {
        Ok(file) => file,
        Err(msg) => return response::bad_request(msg),
    };
    if let Err(msg) = backup::validate_backup(&file) {
        return response::bad_request(msg);
    }

    let lang = state.settings.lang().await;
    // 值级校验复用各路由的创建/更新口径（extra JSON 对象、代理地址格式、
    // 协议/付费枚举、设置按声明类型校验），避免备份导入绕过创建接口的校验。
    if let Some(msg) = validate_import_values(&file, lang) {
        return response::bad_request(msg);
    }

    // 时区变更需要重建全部定时任务（cron 语义时区切换），先记录旧时区。
    let old_timezone = state.settings.timezone().await;
    let summary = match backup::apply_import(&state.db, file).await {
        Ok(summary) => summary,
        Err(msg) => return response::bad_request(msg),
    };

    // 导入成功后同步进程内设置缓存（语言/时区等核心键热生效；
    // AppSettings::update 内部只认它关心的键）。
    let restored = {
        use sea_orm::{EntityTrait, QueryOrder};
        match crate::entity::setting::Entity::find()
            .order_by_asc(crate::entity::setting::Column::Key)
            .all(&state.db)
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!("Failed to reload settings after backup import: {}", e);
                Vec::new()
            }
        }
    };
    for row in &restored {
        state.settings.update(&row.key, &row.value).await;
    }

    // 时区变了则用新时区重建并重算 next_run_at（与设置页改时区同口径）。
    let new_timezone = state.settings.timezone().await;
    if old_timezone != new_timezone {
        let repo = crate::cron::repository::SeaOrmCronJobRepository::new(state.db.clone());
        if let Err(e) = state.scheduler.reload_all_jobs(&repo).await {
            tracing::error!("Failed to reload cron jobs after backup import: {}", e);
        }
    }

    tracing::info!(
        providers = summary.providers,
        models = summary.models,
        virtual_models = summary.virtual_models,
        api_keys = summary.api_keys,
        settings = summary.settings,
        "备份导入完成"
    );

    (StatusCode::OK, Json(Response::success(summary)))
}

/// 备份值级校验（与各业务创建/更新接口同口径），返回第一个错误（None 表示通过）。
/// 结构级校验（自然键唯一、引用可解析、枚举范围）已由 [`backup::validate_backup`] 完成，
/// 这里只补值格式与设置声明类型层面的校验。
fn validate_import_values(file: &BackupFile, lang: Lang) -> Option<String> {
    for (pi, p) in file.providers.iter().enumerate() {
        let loc = format!("providers[{pi}]");
        if let Some(msg) = validate_protocol_billing(p.protocol_type, p.billing_mode, lang) {
            return Some(format!("{loc}：{msg}"));
        }
        if let Some(msg) = validate_proxy(p.proxy_enabled, &p.proxy_addr, lang) {
            return Some(format!("{loc}：{msg}"));
        }
        if let Some(msg) = validate_json_field(
            lang.tr("自定义请求头", "custom headers"),
            &p.custom_header,
            lang,
        ) {
            return Some(format!("{loc}：{msg}"));
        }
        if let Some(msg) = validate_extra(&p.extra, lang) {
            return Some(format!("{loc}：{msg}"));
        }
        for (mi, m) in p.models.iter().enumerate() {
            let m_loc = format!("{loc}.models[{mi}]");
            if let Some(msg) = validate_proxy(m.proxy_enabled, &m.proxy_addr, lang) {
                return Some(format!("{m_loc}：{msg}"));
            }
        }
    }

    for (si, s) in file.settings.iter().enumerate() {
        let t = match s.r#type.parse::<crate::entity::setting::SettingType>() {
            Ok(t) => t as i32,
            Err(_) => continue, // 类型名非法已在结构校验阶段拦截。
        };
        if let Err(msg) = validate_setting_value(t, &s.key, &s.value, lang) {
            return Some(format!("settings[{si}]：{msg}"));
        }
    }

    None
}
