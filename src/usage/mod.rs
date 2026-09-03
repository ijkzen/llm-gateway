//! Provider 用量查询：按 base_url host 分发到各厂商 fetcher，归一化输出。
//!
//! 入口 `query_provider_usage`：校验 provider 存在且 extra.usage 开启 → 查 60s
//! 内存缓存（`?refresh=1` 绕过）→ 调对应 fetcher → 成功结果写缓存。

pub mod cookiecloud;
pub mod error;
pub mod fetchers;
pub mod http;
pub mod persist;
pub mod types;
pub mod volcengine_sign;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::crypto;
use crate::entity::provider;
use error::UsageError;
use fetchers::Credentials;
use types::{FetchOutput, UsageData, UsageKind};

/// 成功结果的缓存时长。
const CACHE_TTL: Duration = Duration::from_secs(60);

/// 用量查询结果缓存（按 provider id；仅缓存成功结果）。
#[derive(Clone, Default)]
pub struct UsageCache {
    inner: Arc<Mutex<HashMap<i32, (Instant, UsageData)>>>,
}

impl UsageCache {
    pub async fn get(&self, provider_id: i32) -> Option<UsageData> {
        let guard = self.inner.lock().await;
        match guard.get(&provider_id) {
            Some((at, data)) if at.elapsed() < CACHE_TTL => Some(data.clone()),
            _ => None,
        }
    }

    pub async fn put(&self, provider_id: i32, data: UsageData) {
        self.inner
            .lock()
            .await
            .insert(provider_id, (Instant::now(), data));
    }

    /// provider 更新/删除后调用，避免旧凭据的缓存结果残留。
    pub async fn invalidate(&self, provider_id: i32) {
        self.inner.lock().await.remove(&provider_id);
    }
}

/// provider 的 extra JSON 是否开启用量查询（`usage: true`）。
pub fn usage_enabled(extra: &str) -> bool {
    let plain = crate::crypto::decrypt_or_passthrough(extra);
    serde_json::from_str::<Value>(&plain)
        .ok()
        .and_then(|v| v.get("usage").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// 查询指定 provider 的用量。`force_refresh` 绕过 60s 缓存。
pub async fn query_provider_usage(
    db: &DatabaseConnection,
    cache: &UsageCache,
    provider_id: i32,
    force_refresh: bool,
) -> Result<UsageData, UsageError> {
    let model = provider::Entity::find_by_id(provider_id)
        .one(db)
        .await
        .map_err(|e| UsageError::Network(format!("数据库查询失败：{e}")))?
        .ok_or(UsageError::Unsupported)?; // 路由层先查存在性，这里兜底

    let extra_plain = crate::crypto::decrypt_or_passthrough(&model.extra);
    let extra = match serde_json::from_str::<Value>(&extra_plain) {
        Ok(Value::Object(map)) => map,
        _ => Default::default(),
    };
    if !usage_enabled(&extra_plain) {
        return Err(UsageError::NotEnabled);
    }

    if !force_refresh && let Some(cached) = cache.get(provider_id).await {
        return Ok(cached);
    }

    let api_key = crypto::decrypt(&model.api_key).unwrap_or_default();
    let creds = Credentials {
        api_key: &api_key,
        extra: &extra,
    };
    let host = crate::provider_template::host_of(&model.base_url).ok_or(UsageError::Unsupported)?;
    let fetcher = fetcher_for(&host, &path_of(&model.base_url)).ok_or(UsageError::Unsupported)?;

    let http = if model.proxy_enabled && !model.proxy_addr.trim().is_empty() {
        http::UsageHttp::with_proxy(Some(&model.proxy_addr))
    } else {
        http::UsageHttp::new()
    };
    let mut rotated_refresh_token = None;
    let output = fetcher
        .fetch(&http, &creds, &mut rotated_refresh_token)
        .await?;
    // 轮换出的新 refresh_token 立即写回；写回失败会作废凭据链，必须报错。
    if let Some(token) = rotated_refresh_token {
        write_back_refresh_token(db, provider_id, &token).await?;
    }
    let data = match output {
        FetchOutput::Quota { plan, windows } => UsageData {
            provider_id,
            fetched_at: chrono::Utc::now(),
            kind: UsageKind::Quota,
            plan,
            windows,
            balances: vec![],
        },
        FetchOutput::Balance { items } => UsageData {
            provider_id,
            fetched_at: chrono::Utc::now(),
            kind: UsageKind::Balance,
            plan: None,
            windows: vec![],
            balances: items,
        },
    };
    cache.put(provider_id, data.clone()).await;
    Ok(data)
}

/// 把轮换出的新 refresh_token 写回 provider extra（只改该键，其余保留）。
/// 写回时重读最新行再合并，缩小与并发 extra 编辑之间的丢更新窗口。
/// 存储值无法解密（如密钥变更）时返回 Err，避免在密文上解析失败后
/// 只留下 refresh_token 而清空其余凭据。
async fn write_back_refresh_token(
    db: &DatabaseConnection,
    provider_id: i32,
    token: &str,
) -> Result<(), UsageError> {
    let latest = provider::Entity::find_by_id(provider_id)
        .one(db)
        .await
        .map_err(|e| UsageError::Network(format!("写回 refresh_token 失败：{e}")))?
        .ok_or(UsageError::Auth)?;
    let extra_plain = crate::crypto::decrypt(&latest.extra).map_err(|_| UsageError::Auth)?;
    let mut map = match serde_json::from_str::<Value>(&extra_plain) {
        Ok(Value::Object(map)) => map,
        _ => Default::default(),
    };
    map.insert(
        "refresh_token".to_string(),
        Value::String(token.to_string()),
    );
    let am = provider::ActiveModel {
        id: Set(provider_id),
        extra: Set(crate::crypto::encrypt(&Value::Object(map).to_string())),
        ..Default::default()
    };
    am.update(db)
        .await
        .map_err(|e| UsageError::Network(format!("写回 refresh_token 失败：{e}")))?;
    Ok(())
}

/// 提取 base_url 的路径部分（小写，无路径返回 "/"）。
fn path_of(base_url: &str) -> String {
    let rest = base_url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(base_url);
    rest.find('/')
        .map(|i| rest[i..].to_ascii_lowercase())
        .unwrap_or_else(|| "/".to_string())
}

/// 按 base_url 的 host（必要时看 path）选择 fetcher。
///
/// 火山方舟与阶跃的「按量付费」和「订阅 Plan」共用 host，靠 path 区分：
/// - ark：path 含 `/api/coding` → Coding Plan（额度窗口），否则 → 费用中心余额
/// - stepfun：path 含 `/step_plan` → Step Plan（cookie 窗口），否则 → 账户余额
fn fetcher_for(host: &str, path: &str) -> Option<Fetcher> {
    Some(match host {
        "opencode.ai" => Fetcher::OpenCodeGo,
        "api.kimi.com" => Fetcher::Kimi,
        "open.bigmodel.cn" => Fetcher::Zhipu { intl: false },
        "api.z.ai" => Fetcher::Zhipu { intl: true },
        "api.minimaxi.com" => Fetcher::Minimax { intl: false },
        "api.minimax.io" => Fetcher::Minimax { intl: true },
        "zenmux.ai" => Fetcher::Zenmux,
        "api.commandcode.ai" => Fetcher::CommandCode,
        "api.deepseek.com" => Fetcher::Deepseek,
        "api.moonshot.ai" => Fetcher::Moonshot { intl: true },
        "api.moonshot.cn" => Fetcher::Moonshot { intl: false },
        "openrouter.ai" => Fetcher::Openrouter,
        "api.githubcopilot.com" => Fetcher::Copilot,
        "ark.cn-beijing.volces.com" => {
            if path.contains("/api/coding") {
                Fetcher::Volcengine
            } else {
                Fetcher::VolcengineBilling
            }
        }
        "dashscope.aliyuncs.com" | "dashscope-intl.aliyuncs.com" => Fetcher::AliyunBss,
        "api.xiaomimimo.com" => Fetcher::XiaomiBalance,
        "api.stepfun.com" | "api.stepfun.ai" => {
            if path.contains("/step_plan") {
                Fetcher::Stepfun
            } else {
                Fetcher::StepfunBalance {
                    intl: host.ends_with(".ai"),
                }
            }
        }
        "coding.dashscope.aliyuncs.com" => Fetcher::AlibabaCoding { intl: false },
        "coding-intl.dashscope.aliyuncs.com" => Fetcher::AlibabaCoding { intl: true },
        "token-plan.cn-beijing.maas.aliyuncs.com" => Fetcher::AlibabaToken { intl: false },
        "token-plan.ap-southeast-1.maas.aliyuncs.com" => Fetcher::AlibabaToken { intl: true },
        _ if host.starts_with("token-plan-") && host.ends_with(".xiaomimimo.com") => {
            Fetcher::XiaomiTokenPlan
        }
        "token.sensenova.cn" | "platform.sensenova.cn" => Fetcher::Sensenova,
        _ => return None,
    })
}

enum Fetcher {
    OpenCodeGo,
    Kimi,
    Zhipu { intl: bool },
    Minimax { intl: bool },
    Zenmux,
    CommandCode,
    Deepseek,
    Moonshot { intl: bool },
    Openrouter,
    Copilot,
    Volcengine,
    VolcengineBilling,
    AliyunBss,
    XiaomiBalance,
    XiaomiTokenPlan,
    Stepfun,
    StepfunBalance { intl: bool },
    AlibabaCoding { intl: bool },
    AlibabaToken { intl: bool },
    Sensenova,
}

impl Fetcher {
    async fn fetch(
        &self,
        http: &http::UsageHttp,
        creds: &Credentials<'_>,
        rotated_refresh_token: &mut Option<String>,
    ) -> Result<FetchOutput, UsageError> {
        use fetchers::{
            alibaba, api_key, balance, cloud_balance, copilot, sensenova, stepfun, volcengine,
            xiaomi,
        };
        match self {
            Fetcher::OpenCodeGo => api_key::fetch_opencode_go(http, creds).await,
            Fetcher::Kimi => api_key::fetch_kimi(http, creds).await,
            Fetcher::Zhipu { intl } => {
                let host = if *intl {
                    "api.z.ai"
                } else {
                    "open.bigmodel.cn"
                };
                api_key::fetch_zhipu(http, creds, host).await
            }
            Fetcher::Minimax { intl } => {
                let host = if *intl {
                    "api.minimax.io"
                } else {
                    "api.minimaxi.com"
                };
                api_key::fetch_minimax(http, creds, host).await
            }
            Fetcher::Zenmux => api_key::fetch_zenmux(http, creds).await,
            Fetcher::CommandCode => api_key::fetch_command_code(http, creds).await,
            Fetcher::Deepseek => balance::fetch_deepseek(http, creds).await,
            Fetcher::Moonshot { intl } => {
                let host = if *intl {
                    "api.moonshot.ai"
                } else {
                    "api.moonshot.cn"
                };
                balance::fetch_moonshot(http, creds, host).await
            }
            Fetcher::Openrouter => balance::fetch_openrouter(http, creds).await,
            Fetcher::Copilot => copilot::fetch_copilot(http, creds).await,
            Fetcher::Volcengine => volcengine::fetch_volcengine(http, creds).await,
            Fetcher::VolcengineBilling => {
                cloud_balance::fetch_volcengine_billing(http, creds).await
            }
            Fetcher::AliyunBss => cloud_balance::fetch_aliyun_bss(http, creds).await,
            Fetcher::XiaomiBalance => xiaomi::fetch_xiaomi_balance(http, creds).await,
            Fetcher::XiaomiTokenPlan => xiaomi::fetch_xiaomi_token_plan(http, creds).await,
            Fetcher::Stepfun => stepfun::fetch_stepfun(http, creds).await,
            Fetcher::StepfunBalance { intl } => {
                let host = if *intl {
                    "api.stepfun.ai"
                } else {
                    "api.stepfun.com"
                };
                balance::fetch_stepfun_account(http, creds, host).await
            }
            Fetcher::AlibabaCoding { intl } => {
                alibaba::fetch_alibaba_coding(http, creds, *intl).await
            }
            Fetcher::AlibabaToken { intl } => {
                alibaba::fetch_alibaba_token(http, creds, *intl).await
            }
            Fetcher::Sensenova => {
                let (output, rotated) = sensenova::fetch_sensenova(http, creds).await?;
                *rotated_refresh_token = rotated;
                Ok(output)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::ENCRYPTION_KEY_ENV;

    #[test]
    fn host_dispatch_covers_seed_templates() {
        for (host, expect_some) in [
            ("opencode.ai", true),
            ("api.kimi.com", true),
            ("open.bigmodel.cn", true),
            ("api.z.ai", true),
            ("api.minimaxi.com", true),
            ("api.minimax.io", true),
            ("zenmux.ai", true),
            ("api.commandcode.ai", true),
            ("api.deepseek.com", true),
            ("api.moonshot.ai", true),
            ("api.moonshot.cn", true),
            ("openrouter.ai", true),
            ("api.githubcopilot.com", true),
            ("ark.cn-beijing.volces.com", true),
            ("dashscope.aliyuncs.com", true),
            ("dashscope-intl.aliyuncs.com", true),
            ("api.xiaomimimo.com", true),
            ("token-plan-cn.xiaomimimo.com", true),
            ("token-plan-ams.xiaomimimo.com", true),
            ("token-plan-sgp.xiaomimimo.com", true),
            ("api.stepfun.com", true),
            ("api.stepfun.ai", true),
            ("coding.dashscope.aliyuncs.com", true),
            ("coding-intl.dashscope.aliyuncs.com", true),
            ("token-plan.cn-beijing.maas.aliyuncs.com", true),
            ("token-plan.ap-southeast-1.maas.aliyuncs.com", true),
            ("api.302.ai", false),
            ("dashscope.aliyuncs.com.evil.com", false),
        ] {
            assert_eq!(fetcher_for(host, "/").is_some(), expect_some, "host={host}");
        }
    }

    #[test]
    fn volcengine_and_stepfun_dispatch_by_path() {
        // 火山：coding 路径 → 订阅窗口；其余 → 账户余额
        assert!(matches!(
            fetcher_for("ark.cn-beijing.volces.com", "/api/coding/v3"),
            Some(Fetcher::Volcengine)
        ));
        assert!(matches!(
            fetcher_for("ark.cn-beijing.volces.com", "/api/v3"),
            Some(Fetcher::VolcengineBilling)
        ));
        // 阶跃：step_plan 路径 → 订阅窗口（cookie）；其余 → 账户余额（API key）
        assert!(matches!(
            fetcher_for("api.stepfun.com", "/step_plan/v1"),
            Some(Fetcher::Stepfun)
        ));
        assert!(matches!(
            fetcher_for("api.stepfun.com", "/v1"),
            Some(Fetcher::StepfunBalance { intl: false })
        ));
        assert!(matches!(
            fetcher_for("api.stepfun.ai", "/v1"),
            Some(Fetcher::StepfunBalance { intl: true })
        ));
    }

    #[tokio::test]
    async fn cache_ttl_and_invalidate() {
        let cache = UsageCache::default();
        assert!(cache.get(1).await.is_none());
        let data = UsageData {
            provider_id: 1,
            fetched_at: chrono::Utc::now(),
            kind: UsageKind::Balance,
            plan: None,
            windows: vec![],
            balances: vec![],
        };
        cache.put(1, data).await;
        assert!(cache.get(1).await.is_some());
        cache.invalidate(1).await;
        assert!(cache.get(1).await.is_none());
    }

    #[test]
    fn usage_enabled_handles_encrypted_extra() {
        temp_env::with_vars([(ENCRYPTION_KEY_ENV, Some("test-key"))], || {
            let plain = r#"{"usage": true, "usage_type": 0}"#;
            let encrypted = crate::crypto::encrypt(plain);
            assert!(usage_enabled(&encrypted), "密文 extra 应判读 usage=true");
            assert!(usage_enabled(plain), "明文 extra 行为不变");
            assert!(!usage_enabled(&crate::crypto::encrypt(
                r#"{"usage": false}"#
            )));
            // 解密失败（如密钥变更）时安全降级为未开启。
            assert!(!usage_enabled("enc:v1:broken"));
        });
    }

    #[tokio::test]
    async fn write_back_refresh_token_preserves_encryption() {
        temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some("test-key"))], async {
            let db = crate::db::connect("sqlite::memory:").await.unwrap();
            let now = chrono::Utc::now();
            let extra_plain = r#"{"refresh_token":"old-token","usage":true}"#;
            let p = crate::entity::provider::ActiveModel {
                name: Set("Sensenova".to_string()),
                enable: Set(true),
                base_url: Set("https://token.sensenova.cn/v1".to_string()),
                api_key: Set(crate::crypto::encrypt("sk-x")),
                custom_header: Set("{}".to_string()),
                protocol_type: Set(0),
                billing_mode: Set(1),
                extra: Set(crate::crypto::encrypt(extra_plain)),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();

            write_back_refresh_token(&db, p.id, "new-token")
                .await
                .unwrap();

            let row = crate::entity::provider::Entity::find_by_id(p.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            assert!(row.extra.starts_with("enc:v1:"), "写回后 extra 仍应为密文");
            let decrypted = crate::crypto::decrypt(&row.extra).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&decrypted).unwrap();
            assert_eq!(parsed["refresh_token"], "new-token");
            assert_eq!(parsed["usage"], true, "其余键保留");
        })
        .await;
    }
}
