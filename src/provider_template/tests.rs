use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, Set,
};

use super::{find_by_domain, find_by_domain_all, seed, upsert_templates};
use crate::entity::provider_template::{self, Entity};

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
    use crate::entity::provider;
    let now = chrono::Utc::now();
    provider::ActiveModel {
        name: Set(name.to_string()),
        base_url: Set(base_url.to_string()),
        api_key: Set(String::new()),
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
    use crate::entity::provider;
    let row = provider::Entity::find()
        .filter(provider::Column::Name.eq(name))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&row.extra).unwrap()
}

#[tokio::test]
async fn test_first_insert_backfills_matching_provider_extra() {
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
}

#[tokio::test]
async fn test_backfill_only_on_first_insert_not_on_update() {
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
}
