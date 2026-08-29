use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter,
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
    for expect in ["DeepSeek", "OpenRouter", "Moonshot AI", "Zhipu AI Coding Plan", "OpenCode Go", "Command Code"] {
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
    let found = find_by_domain(&db, "https://api.deepseek.com/v1/chat").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "DeepSeek");
}

#[tokio::test]
async fn test_find_by_domain_all_returns_all_hits() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    // api.stepfun.com host 下有按量 StepFun (China) 与订阅 StepFun Step Plan (China) 两个模板。
    let all = find_by_domain_all(&db, "api.stepfun.com").await.unwrap();
    assert!(all.len() >= 2, "同一 host 应返回全部命中，实际 {}", all.len());
    assert!(all.iter().any(|t| t.name == "StepFun (China)"));
    assert!(all.iter().any(|t| t.name == "StepFun Step Plan (China)"));

    // find_by_domain 是第一条命中，行为不变。
    let first = find_by_domain(&db, "api.stepfun.com").await.unwrap();
    assert!(first.is_some());

    // 无命中返回空列表。
    assert!(find_by_domain_all(&db, "nonexistent.example.com").await.unwrap().is_empty());
    assert!(find_by_domain_all(&db, "").await.unwrap().is_empty());
}

#[tokio::test]
async fn test_find_by_domain_no_match_returns_none() {
    let db = setup_db().await.unwrap();
    upsert_templates(&db).await.unwrap();

    // 不存在/含占位符的域名不匹配
    assert!(find_by_domain(&db, "nonexistent.example.com").await.unwrap().is_none());
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
    assert_eq!(host_of("https://api.deepseek.com"), Some("api.deepseek.com".to_string()));
    assert_eq!(host_of("https://api.302.ai/v1"), Some("api.302.ai".to_string()));
    assert_eq!(host_of("http://localhost:8080/v1"), Some("localhost".to_string()));
    assert_eq!(host_of("${CLOUDFLARE_ACCOUNT_ID}/ai/v1"), None);
    assert_eq!(host_of(""), None);
}
