mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use tower::ServiceExt;

use llm_gateway::app_settings::AppSettings;
use llm_gateway::cron::repository::{CronJobRepository, JobDefinition, SeaOrmCronJobRepository};
use llm_gateway::entity::setting;

async fn put_setting(app: &axum::Router, key: &str, value: &str) -> axum::response::Response {
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri(format!("/api/settings/{key}"))
        .header("content-type", "application/json")
        .body(Body::from(format!(r#"{{"value":"{value}"}}"#)))
        .unwrap();
    app.clone().oneshot(request).await.unwrap()
}

/// 在测试库中种入 language / timezone 设置行（模拟已初始化并选择过语言的库）。
async fn seed_settings(db: &sea_orm::DatabaseConnection, language: &str, timezone: &str) {
    for (key, value) in [("language", language), ("timezone", timezone)] {
        let exists = setting::Entity::find_by_id(key).one(db).await.unwrap();
        if exists.is_some() {
            continue;
        }
        setting::ActiveModel {
            key: Set(key.to_string()),
            value: Set(value.to_string()),
            r#type: Set(0),
            updated_at: Set(chrono::Utc::now()),
        }
        .insert(db)
        .await
        .unwrap();
    }
}

/// 与 build_authed_app 相同，但 AppSettings 从库里加载指定语言/时区。
async fn build_app_with_settings(
    language: &str,
    timezone: &str,
) -> (axum::Router, sea_orm::DatabaseConnection, AppSettings) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    seed_settings(&db, language, timezone).await;
    let settings = AppSettings::load_from_db(&db).await.unwrap();
    scheduler.start().await.unwrap();
    let app =
        common::build_authed_app_with_settings(db.clone(), scheduler, log_tx, settings.clone())
            .await;
    (app, db, settings)
}

#[tokio::test]
async fn test_api_error_messages_follow_language_setting() {
    // 语言 = en：错误消息应为英文。
    let (app, _db, _settings) = build_app_with_settings("en", "UTC").await;

    // 创建一个缺 name 的 Provider → 400 "name cannot be empty"
    let request: Request<Body> = Request::builder()
        .method("POST")
        .uri("/api/providers")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"","enable":true,"baseUrl":"https://x.ai/v1","apiKey":"sk-test","protocolType":0,"billingMode":0,"customHeader":"{}","extra":"{}"}"#,
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 400);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        body_str.contains("name cannot be empty"),
        "expected English message, got: {body_str}"
    );

    // 设置项 key 不存在 → 404 "setting ... does not exist"
    let response = put_setting(&app, "no_such_key", "x").await;
    assert_eq!(response.status(), 404);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("does not exist"), "got: {body_str}");
}

#[tokio::test]
async fn test_api_error_messages_default_chinese() {
    // 默认（语言 = zh-CN）：错误消息保持中文，与既有断言一致。
    let (app, _db, _settings) = build_app_with_settings("zh-CN", "Asia/Shanghai").await;

    let request: Request<Body> = Request::builder()
        .method("POST")
        .uri("/api/providers")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"","enable":true,"baseUrl":"https://x.ai/v1","apiKey":"sk-test","protocolType":0,"billingMode":0,"customHeader":"{}","extra":"{}"}"#,
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 400);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("名称不能为空"), "got: {body_str}");
}

#[tokio::test]
async fn test_put_language_updates_process_settings_cache() {
    let (app, db, settings) = build_app_with_settings("zh-CN", "Asia/Shanghai").await;

    // 初始语言 zh。
    assert_eq!(settings.lang().await, llm_gateway::i18n::Lang::Zh);

    // PUT language=en → 缓存与数据库同步更新。
    let response = put_setting(&app, "language", "en").await;
    assert_eq!(response.status(), 200);
    assert_eq!(settings.lang().await, llm_gateway::i18n::Lang::En);

    let model = setting::Entity::find_by_id("language")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "en");

    // 非法语言被拒绝。
    let response = put_setting(&app, "language", "fr").await;
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn test_put_timezone_reloads_cron_jobs_and_recomputes_next_run() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    seed_settings(&db, "zh-CN", "Asia/Shanghai").await;
    let settings = AppSettings::load_from_db(&db).await.unwrap();

    // 注册 handler 并种一个 cron 任务（每天 08:00）。
    scheduler
        .register_handler(
            "tz_reload_job",
            std::sync::Arc::new(|_ctx: llm_gateway::cron::JobContext| {
                Box::pin(async move { Ok(()) })
            }),
        )
        .await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let job = JobDefinition {
        name: "tz_reload_job".to_string(),
        title: "TZ Reload".to_string(),
        description: "".to_string(),
        expression: "0 0 8 * * *".to_string(),
        enabled: true,
        group: "default".to_string(),
    };
    repo.insert(&job, Some(chrono_tz::Asia::Shanghai))
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();
    scheduler.start().await.unwrap();

    let app =
        common::build_authed_app_with_settings(db.clone(), scheduler, log_tx, settings.clone())
            .await;
    let app_clone = app.clone();

    let before = repo.find_by_name("tz_reload_job").await.unwrap().unwrap();
    let before_next = before.next_run_at;

    // 切换时区 → 任务重建、next_run_at 重算（Asia/Tokyo 比 Shanghai 早 1 小时，
    // 计算出的下次 08:00 对应 UTC 时刻应变化）。
    let response = put_setting(&app, "timezone", "Asia/Tokyo").await;
    assert_eq!(response.status(), 200);

    let after = repo.find_by_name("tz_reload_job").await.unwrap().unwrap();
    assert_ne!(
        after.next_run_at, before_next,
        "next_run_at should be recomputed after timezone change"
    );

    // 任务仍在调度器里（可手动执行）。
    let request: Request<Body> = Request::builder()
        .method("POST")
        .uri("/api/cron-jobs/tz_reload_job/run")
        .body(Body::empty())
        .unwrap();
    let response = app_clone.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    // 非法时区被拒绝。
    let response = put_setting(&app, "timezone", "Mars/Olympus").await;
    assert_eq!(response.status(), 400);
    let model = setting::Entity::find_by_id("timezone")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "Asia/Tokyo");
}

#[tokio::test]
async fn test_seed_rows_exist_for_fresh_db() {
    // 全新空库：load_from_db 幂等补种子行，PUT 不再 404。
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let settings = AppSettings::load_from_db(&db).await.unwrap();
    scheduler.start().await.unwrap();
    let app =
        common::build_authed_app_with_settings(db.clone(), scheduler, log_tx, settings.clone())
            .await;

    let language = setting::Entity::find_by_id("language")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(language.value, "zh-CN");
    let timezone = setting::Entity::find_by_id("timezone")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(timezone.value, "Asia/Shanghai");

    // 种子行可正常更新。
    let response = put_setting(&app, "language", "en").await;
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_timezone_must_be_valid_iana() {
    let (app, db, _settings) = build_app_with_settings("zh-CN", "Asia/Shanghai").await;

    let response = put_setting(&app, "timezone", "not-a-timezone").await;
    assert_eq!(response.status(), 400);
    let model = setting::Entity::find_by_id("timezone")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "Asia/Shanghai");
}

#[tokio::test]
async fn test_usage_balance_label_localization() {
    use llm_gateway::i18n::Lang;
    use llm_gateway::usage::types::BalanceItem;

    let item = BalanceItem {
        label: "余额（CNY）".to_string(),
        amount: 1.23,
        currency: Some("CNY".to_string()),
    };
    // 默认 zh 输出与旧版一致。
    assert_eq!(item.with_localized_label(Lang::Zh).label, "余额（CNY）");
    // 英文翻译 base 并重拼。
    assert_eq!(item.with_localized_label(Lang::En).label, "Balance (CNY)");

    let item = BalanceItem {
        label: "剩余额度".to_string(),
        amount: 10.0,
        currency: None,
    };
    assert_eq!(
        item.with_localized_label(Lang::En).label,
        "Remaining Credits"
    );
}
