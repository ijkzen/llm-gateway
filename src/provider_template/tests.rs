use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, Set,
};

use super::{find_by_domain_all, seed, upsert_templates};
use crate::entity::provider_template::{self, Entity};

/// find_by_domain 已删（无生产调用），测试内联等价实现。
async fn find_by_domain(
    db: &DatabaseConnection,
    domain: &str,
) -> Result<Option<provider_template::Model>, sea_orm::DbErr> {
    Ok(find_by_domain_all(db, domain).await?.into_iter().next())
}

/// 建一个内存 SQLite 连接（表由 migrate 建好）。
async fn setup_db() -> Result<DatabaseConnection, DbErr> {
    crate::db::connect("sqlite::memory:").await
}

#[tokio::test]
async fn test_upsert_inserts_all_templates() {
    let db = setup_db().await.unwrap();

    let n = upsert_templates(&db).await.unwrap();
    assert_eq!(n, seed::TEMPLATES.len());

    let count = Entity::find().count(&db).await.unwrap();
    assert_eq!(count as usize, seed::TEMPLATES.len());
}

#[tokio::test]
async fn test_upsert_is_idempotent() {
    let db = setup_db().await.unwrap();

    upsert_templates(&db).await.unwrap();
    let n2 = upsert_templates(&db).await.unwrap();
    // 第二次全为更新，不新增
    assert_eq!(n2, seed::TEMPLATES.len());

    let count = Entity::find().count(&db).await.unwrap();
    assert_eq!(count as usize, seed::TEMPLATES.len());
}

#[tokio::test]
async fn test_upsert_updates_existing_template() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    // 模拟用户修改：把某条模板的 base_url 改掉
    let tmpl = &seed::TEMPLATES[0];
    let row = Entity::find()
        .filter(provider_template::Column::Name.eq(tmpl.name))
        .one(&db)
        .await
        .unwrap()
        .expect("template should exist");
    let mut am: provider_template::ActiveModel = row.into();
    am.base_url = sea_orm::Set("https://example.com/changed".to_string());
    am.update(&db).await.unwrap();

    // 再次 upsert，应恢复到种子值
    upsert_templates(&db).await.unwrap();
    let row2 = Entity::find()
        .filter(provider_template::Column::Name.eq(tmpl.name))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row2.base_url, tmpl.base_url);
}

#[tokio::test]
async fn test_seed_has_expected_entries() {
    let names: Vec<&str> = seed::TEMPLATES.iter().map(|t| t.name).collect();
    // 关键 provider 应存在
    for expect in [
        "DeepSeek",
        "OpenRouter",
        "Moonshot AI",
        "Zhipu AI Coding Plan",
        "OpenCode Go",
        "Command Code",
        "SenseNova",
    ] {
        assert!(names.contains(&expect), "missing {expect}");
    }
    // 无 base_url 的 provider 不应存在（Claude/Codex/Kimi 会员等）
    for absent in ["Claude", "Codex", "Kimi 会员", "Qoder", "Cursor"] {
        assert!(!names.contains(&absent), "unexpected {absent}");
    }
    // extra 至少要有 cookie 类和 oauth 类
    let extras: Vec<&str> = seed::TEMPLATES.iter().map(|t| t.extra).collect();
    assert!(extras.iter().any(|e| e.contains("cookie_cloud_server")));
    assert!(extras.iter().any(|e| e.contains("oauth_token")));
    assert!(extras.iter().any(|e| e.contains("\"ak\"")));
    // 支持用量查询的 provider 带 usage/usage_type（0=余额，1=月剩余额度/百分比）
    let usage_extras: Vec<&str> = extras
        .iter()
        .filter(|e| e.contains("\"usage\": true"))
        .copied()
        .collect();
    assert!(!usage_extras.is_empty(), "no provider marks usage support");
    assert!(
        usage_extras.iter().any(|e| e.contains("\"usage_type\": 0")),
        "no balance-type (usage_type=0) provider"
    );
    assert!(
        usage_extras.iter().any(|e| e.contains("\"usage_type\": 1")),
        "no quota-percent-type (usage_type=1) provider"
    );
}

#[tokio::test]
async fn test_find_by_domain_matches_host() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    let found = find_by_domain(&db, "api.deepseek.com").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "DeepSeek");

    // 带端口也能匹配
    let found = find_by_domain(&db, "api.deepseek.com:443").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "DeepSeek");
}

#[tokio::test]
async fn test_find_by_domain_ignores_case_and_path() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    // 大小写不敏感
    let found = find_by_domain(&db, "API.DEEPSEEK.COM").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "DeepSeek");

    // 带路径/协议
    let found = find_by_domain(&db, "https://api.deepseek.com/v1/chat")
        .await
        .unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "DeepSeek");
}

#[tokio::test]
async fn test_find_by_domain_all_returns_all_hits() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    // api.stepfun.com host 下有按量 StepFun (China) 与订阅 StepFun Step Plan (China) 两个模板。
    let all = find_by_domain_all(&db, "api.stepfun.com").await.unwrap();
    assert!(
        all.len() >= 2,
        "同一 host 应返回全部命中，实际 {}",
        all.len()
    );
    assert!(all.iter().any(|t| t.name == "StepFun (China)"));
    assert!(all.iter().any(|t| t.name == "StepFun Step Plan (China)"));

    // find_by_domain 是第一条命中，行为不变。
    let first = find_by_domain(&db, "api.stepfun.com").await.unwrap();
    assert!(first.is_some());

    // 无命中返回空列表。
    assert!(
        find_by_domain_all(&db, "nonexistent.example.com")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(find_by_domain_all(&db, "").await.unwrap().is_empty());
}

#[tokio::test]
async fn test_find_by_domain_no_match_returns_none() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    // 不存在/含占位符的域名不匹配
    assert!(
        find_by_domain(&db, "nonexistent.example.com")
            .await
            .unwrap()
            .is_none()
    );
    assert!(find_by_domain(&db, "").await.unwrap().is_none());
    assert!(find_by_domain(&db, "  ").await.unwrap().is_none());
    // Cloudflare Workers AI 的 host 是 api.cloudflare.com（路径含 ${VAR} 但 host 干净），
    // 请求打向该域名时应命中此模板。
    let cf = find_by_domain(&db, "api.cloudflare.com").await.unwrap();
    assert!(cf.is_some());
    assert_eq!(cf.unwrap().name, "Cloudflare Workers AI");
}

#[test]
fn test_host_of_extracts_domain() {
    use super::host_of;
    assert_eq!(
        host_of("https://api.deepseek.com"),
        Some("api.deepseek.com".to_string())
    );
    assert_eq!(
        host_of("https://api.302.ai/v1"),
        Some("api.302.ai".to_string())
    );
    assert_eq!(
        host_of("http://localhost:8080/v1"),
        Some("localhost".to_string())
    );
    assert_eq!(host_of("${CLOUDFLARE_ACCOUNT_ID}/ai/v1"), None);
    assert_eq!(host_of(""), None);
}

// ── 模板首次插入时向同 host 既有 provider 回填 extra 缺失键 ──

async fn insert_provider(db: &DatabaseConnection, name: &str, base_url: &str, extra: &str) {
    insert_provider_with_billing(db, name, base_url, extra, 0).await;
}

async fn insert_provider_with_billing(
    db: &DatabaseConnection,
    name: &str,
    base_url: &str,
    extra: &str,
    billing_mode: i32,
) {
    use crate::entity::provider;
    let now = chrono::Utc::now();
    provider::ActiveModel {
        name: Set(name.to_string()),
        base_url: Set(base_url.to_string()),
        api_key: Set(String::new()),
        billing_mode: Set(billing_mode),
        extra: Set(extra.to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

async fn provider_extra(db: &DatabaseConnection, name: &str) -> serde_json::Value {
    use crate::crypto;
    use crate::entity::provider;
    let row = provider::Entity::find()
        .filter(provider::Column::Name.eq(name))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let plain = crypto::decrypt_or_passthrough(&row.extra);
    serde_json::from_str(&plain).unwrap()
}

#[tokio::test]
async fn krill_seed_has_payg_and_subscription_templates() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    let templates = find_by_domain_all(&db, "api-slb.krill-ai.net")
        .await
        .unwrap();
    assert_eq!(templates.len(), 2);
    assert!(templates.iter().any(|t| {
        t.name == "Krill（按量付费）"
            && t.billing_mode == 0
            && t.extra.contains("\"usage_type\": 0")
    }));
    assert!(templates.iter().any(|t| {
        t.name == "Krill（订阅制）" && t.billing_mode == 1 && t.extra.contains("\"usage_type\": 1")
    }));
}

#[tokio::test]
async fn krill_history_backfill_is_idempotent_and_preserves_user_values() {
    temp_env::async_with_vars(
        [(crate::crypto::ENCRYPTION_KEY_ENV, Some("test-key"))],
        async {
            let db = setup_db().await.unwrap();
            insert_provider_with_billing(
                &db,
                "Krill-按量历史",
                "https://api.krill-ai.net/v1",
                r#"{"email":"saved@example.com","custom":"keep"}"#,
                0,
            )
            .await;
            insert_provider_with_billing(
                &db,
                "Krill-订阅历史",
                "https://api.cdn-krill-ai.com/v1",
                r#"{"usage":false,"usage_type":0,"password":"saved-password","jwt":"saved-jwt"}"#,
                1,
            )
            .await;

            upsert_templates(&db).await.unwrap();
            upsert_templates(&db).await.unwrap();

            let payg = provider_extra(&db, "Krill-按量历史").await;
            assert_eq!(payg["email"], "saved@example.com");
            assert_eq!(payg["password"], "");
            assert_eq!(payg["jwt"], "");
            assert_eq!(payg["usage"], true);
            assert_eq!(payg["usage_type"], 0);
            assert_eq!(payg["custom"], "keep");

            let subscription = provider_extra(&db, "Krill-订阅历史").await;
            assert_eq!(subscription["usage"], false);
            assert_eq!(subscription["usage_type"], 1);
            assert_eq!(subscription["email"], "");
            assert_eq!(subscription["password"], "saved-password");
            assert_eq!(subscription["jwt"], "saved-jwt");
        },
    )
    .await;
}

#[tokio::test]
async fn sensenova_history_backfill_is_idempotent_and_preserves_user_values() {
    temp_env::async_with_vars(
        [(crate::crypto::ENCRYPTION_KEY_ENV, Some("test-key"))],
        async {
            let db = setup_db().await.unwrap();
            // 先 upsert 让 SenseNova 模板入库（后续走 update 分支，不再触发首次插入回填）。
            upsert_templates(&db).await.unwrap();

            // 模拟合并前创建的历史 SenseNova provider：extra 只有旧键 refresh_token/usage。
            insert_provider_with_billing(
                &db,
                "SenseNova-历史",
                "https://token.sensenova.cn/v1",
                r#"{"refresh_token":"rt-existing","usage":true,"usage_type":1,"custom":"keep"}"#,
                1,
            )
            .await;
            // 其它 host 的 provider 不受影响。
            insert_provider(
                &db,
                "SenseNova-其他host",
                "https://api.deepseek.com/v1",
                r#"{"own":1}"#,
            )
            .await;

            // 再次 upsert（模板走 update 分支），历史 provider 仍应被无条件对齐。
            upsert_templates(&db).await.unwrap();
            upsert_templates(&db).await.unwrap();

            let extra = provider_extra(&db, "SenseNova-历史").await;
            assert_eq!(extra["username"], "");
            assert_eq!(extra["password"], "");
            assert_eq!(
                extra["refresh_token"], "rt-existing",
                "已有 refresh_token 不被覆盖"
            );
            assert_eq!(extra["usage"], true);
            assert_eq!(extra["custom"], "keep", "未知键保留");

            let other = provider_extra(&db, "SenseNova-其他host").await;
            assert_eq!(
                other,
                serde_json::json!({ "own": 1 }),
                "非 SenseNova host 不动"
            );
        },
    )
    .await;
}

#[tokio::test]
async fn krill_history_backfill_skips_invalid_extra_and_continues() {
    let db = setup_db().await.unwrap();
    insert_provider_with_billing(
        &db,
        "Krill-坏历史",
        "https://api.krill-ai.net/v1",
        "not-json",
        0,
    )
    .await;
    insert_provider_with_billing(
        &db,
        "Krill-好历史",
        "https://api.cdn-krill-ai.com/v1",
        r#"{}"#,
        1,
    )
    .await;

    upsert_templates(&db).await.unwrap();

    assert_eq!(provider_extra(&db, "Krill-好历史").await["usage_type"], 1);
    use crate::entity::provider;
    let invalid = provider::Entity::find()
        .filter(provider::Column::Name.eq("Krill-坏历史"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invalid.extra, "not-json");
}

#[tokio::test]
async fn krill_code_net_host_is_recognized_and_backfilled() {
    // 生产在用 `api-slb.krill-code.net`（与 krill-code.com 控制台同后端），
    // 早期只认 krill-ai.net 导致该 host 的 provider 不被识别、也不回填。
    temp_env::async_with_vars(
        [(crate::crypto::ENCRYPTION_KEY_ENV, Some("test-key"))],
        async {
            let db = setup_db().await.unwrap();
            assert!(super::is_krill_host("api-slb.krill-code.net"));

            insert_provider_with_billing(
                &db,
                "Krill-生产域",
                "https://api-slb.krill-code.net/codex/v1",
                r#"{}"#,
                0,
            )
            .await;

            upsert_templates(&db).await.unwrap();

            let extra = provider_extra(&db, "Krill-生产域").await;
            assert_eq!(extra["email"], "");
            assert_eq!(extra["password"], "");
            assert_eq!(extra["jwt"], "");
            assert_eq!(extra["usage"], true);
            assert_eq!(extra["usage_type"], 0);
        },
    )
    .await;
}

#[tokio::test]
async fn test_first_insert_backfills_matching_provider_extra() {
    temp_env::async_with_vars(
        [(crate::crypto::ENCRYPTION_KEY_ENV, Some("test-key"))],
        async {
            let db = setup_db().await.unwrap();
            // provider 先于模板存在（历史上手动创建的商汤供应商）。
            insert_provider(
                &db,
                "商汤",
                "https://token.sensenova.cn/v1",
                r#"{"custom": "keep"}"#,
            )
            .await;
            // 无模板命中的 host 不动（DeepSeek 等自身带 usage 标记的模板也会回填其 host）。
            insert_provider(&db, "其他", "https://usage.example.com/v1", r#"{"own": 1}"#).await;

            upsert_templates(&db).await.unwrap();

            // 同 host 的既有 provider 补齐模板 extra 缺失键，已有键保留。
            let extra = provider_extra(&db, "商汤").await;
            assert_eq!(extra["custom"], "keep");
            assert_eq!(extra["usage"], true);
            assert_eq!(extra["usage_type"], 1);
            assert_eq!(extra["refresh_token"], "");

            // 非 host 匹配的 provider 不动。
            let other = provider_extra(&db, "其他").await;
            assert_eq!(other, serde_json::json!({ "own": 1 }));
        },
    )
    .await;
}

#[tokio::test]
async fn test_backfill_only_on_first_insert_not_on_update() {
    temp_env::async_with_vars(
        [(crate::crypto::ENCRYPTION_KEY_ENV, Some("test-key"))],
        async {
            let db = setup_db().await.unwrap();
            insert_provider(&db, "商汤", "https://token.sensenova.cn/v1", r#"{}"#).await;
            upsert_templates(&db).await.unwrap();
            assert_eq!(provider_extra(&db, "商汤").await["usage"], true);

            // 模拟用户关闭 usage 后再次 upsert（更新分支）：不回填。
            use crate::entity::provider;
            let row = provider::Entity::find()
                .filter(provider::Column::Name.eq("商汤"))
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            let mut am: provider::ActiveModel = row.into();
            am.extra = Set(r#"{"usage": false}"#.to_string());
            am.update(&db).await.unwrap();

            upsert_templates(&db).await.unwrap();
            assert_eq!(provider_extra(&db, "商汤").await["usage"], false);
        },
    )
    .await;
}
