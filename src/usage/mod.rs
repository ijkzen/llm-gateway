//! Provider 用量查询：按 base_url host 分发到各厂商 fetcher，归一化输出。
//!
//! 入口 `query_provider_usage`：校验 provider 存在且 extra.usage 开启 → 查 60s
//! 内存缓存（`?refresh=1` 绕过）→ 调对应 fetcher → 成功结果写缓存。

pub mod cookiecloud;
pub mod error;
pub mod fetchers;
pub mod http;
pub mod types;
pub mod volcengine_sign;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sea_orm::{DatabaseConnection, EntityTrait};
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

    let extra = match serde_json::from_str::<Value>(&model.extra) {
        Ok(Value::Object(map)) => map,
        _ => Default::default(),
    };
    let usage_enabled = extra
        .get("usage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !usage_enabled {
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
    let host = crate::provider_template::host_of(&model.base_url)
        .ok_or(UsageError::Unsupported)?;
    let fetcher = fetcher_for_host(&host).ok_or(UsageError::Unsupported)?;

    let http = http::UsageHttp::new();
    let output = fetcher.fetch(&http, &creds).await?;

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

/// 按 base_url host 选择 fetcher。仅当 provider 开启了 usage 才会走到这里；
/// host 未收录（自定义网关等）返回 None → Unsupported。
fn fetcher_for_host(host: &str) -> Option<Fetcher> {
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
        "ark.cn-beijing.volces.com" => Fetcher::Volcengine,
        "api.xiaomimimo.com" => Fetcher::XiaomiBalance,
        "api.stepfun.com" | "api.stepfun.ai" => Fetcher::Stepfun,
        "coding.dashscope.aliyuncs.com" => Fetcher::AlibabaCoding { intl: false },
        "coding-intl.dashscope.aliyuncs.com" => Fetcher::AlibabaCoding { intl: true },
        "token-plan.cn-beijing.maas.aliyuncs.com" => Fetcher::AlibabaToken { intl: false },
        "token-plan.ap-southeast-1.maas.aliyuncs.com" => Fetcher::AlibabaToken { intl: true },
        _ if host.starts_with("token-plan-") && host.ends_with(".xiaomimimo.com") => {
            Fetcher::XiaomiTokenPlan
        }
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
    XiaomiBalance,
    XiaomiTokenPlan,
    Stepfun,
    AlibabaCoding { intl: bool },
    AlibabaToken { intl: bool },
}

impl Fetcher {
    async fn fetch(
        &self,
        http: &http::UsageHttp,
        creds: &Credentials<'_>,
    ) -> Result<FetchOutput, UsageError> {
        use fetchers::{alibaba, api_key, balance, copilot, stepfun, volcengine, xiaomi};
        match self {
            Fetcher::OpenCodeGo => api_key::fetch_opencode_go(http, creds).await,
            Fetcher::Kimi => api_key::fetch_kimi(http, creds).await,
            Fetcher::Zhipu { intl } => {
                let host = if *intl { "api.z.ai" } else { "open.bigmodel.cn" };
                api_key::fetch_zhipu(http, creds, host).await
            }
            Fetcher::Minimax { intl } => {
                let host = if *intl { "api.minimax.io" } else { "api.minimaxi.com" };
                api_key::fetch_minimax(http, creds, host).await
            }
            Fetcher::Zenmux => api_key::fetch_zenmux(http, creds).await,
            Fetcher::CommandCode => api_key::fetch_command_code(http, creds).await,
            Fetcher::Deepseek => balance::fetch_deepseek(http, creds).await,
            Fetcher::Moonshot { intl } => {
                let host = if *intl { "api.moonshot.ai" } else { "api.moonshot.cn" };
                balance::fetch_moonshot(http, creds, host).await
            }
            Fetcher::Openrouter => balance::fetch_openrouter(http, creds).await,
            Fetcher::Copilot => copilot::fetch_copilot(http, creds).await,
            Fetcher::Volcengine => volcengine::fetch_volcengine(http, creds).await,
            Fetcher::XiaomiBalance => xiaomi::fetch_xiaomi_balance(http, creds).await,
            Fetcher::XiaomiTokenPlan => xiaomi::fetch_xiaomi_token_plan(http, creds).await,
            Fetcher::Stepfun => stepfun::fetch_stepfun(http, creds).await,
            Fetcher::AlibabaCoding { intl } => alibaba::fetch_alibaba_coding(http, creds, *intl).await,
            Fetcher::AlibabaToken { intl } => alibaba::fetch_alibaba_token(http, creds, *intl).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ("openrouter.ai", true),
            ("api.githubcopilot.com", true),
            ("ark.cn-beijing.volces.com", true),
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
            ("dashscope.aliyuncs.com", false),
        ] {
            assert_eq!(fetcher_for_host(host).is_some(), expect_some, "host={host}");
        }
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
}
