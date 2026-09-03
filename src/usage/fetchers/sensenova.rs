//! SenseNova Token Plan（控制台 OAuth）用量查询 + 登录自愈。
//!
//! 凭据：`extra.refresh_token`（后端派生，续期/登录后写回）为主，`extra.username` +
//! `extra.password`（用户维护）用于 refresh_token 失效时登录换新。
//!
//! 状态机（仿 Krill JWT 自愈）：
//! - `refresh_token` 非空 → 先续期（POST /oauth2/token，form `grant_type=refresh_token`、
//!   `client_id=nova`）→ 用 access_token 查 pool-usage；续期成功把轮换出的新 refresh_token
//!   写回 provider extra。
//! - 仅当 refresh_token **缺失**，或续期**明确鉴权失败**（HTTP 401/403，或 200 但响应
//!   `{"error":"invalid_grant"}` / 解析不出 access_token）→ 用 username/password 走登录
//!   （见 `sensenova_login` 模块）换取新 refresh_token → 先写回 extra → 用新 refresh_token
//!   重试一次续期+查询。
//! - 网络错误 / 5xx / 非认证业务错误 / 解析错误不触发登录；登录失败（密码错/无 redirect）
//!   → `UsageError::Auth`。
//!
//! 查询：GET /lite/console/v1/tokenplan/pool-usage（Bearer access_token），逐积分池产出
//! 5h/7d 窗口（label=池名）；sk- 密钥不能调用量接口（401）。

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

use super::{Credentials, SensenovaContext, num, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, ensure_not_auth_error};
use crate::usage::sensenova_login::SensenovaLogin;
use crate::usage::types::{FetchOutput, QuotaWindow, WindowKind, ts_secs};

const TOKEN_URL: &str = "https://platform.sensenova.cn/oauth2/token";
const USAGE_URL: &str = "https://platform.sensenova.cn/lite/console/v1/tokenplan/pool-usage";
/// 登录失败后的冷却时长：账号密码连续错误会触发商汤账号临时锁定
/// （forbidLoginForMoment，约 10 分钟），冷却期内不再尝试登录，避免
/// usage_refresh 每 5 分钟撞锁把锁定窗口无限刷新。
const LOGIN_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// 冷却期内的错误文案（不再打登录接口，直接返回该语义）。
const LOGIN_COOLDOWN_MSG: &str = "商汤账号登录失败次数过多，已临时冷却，请稍后重试";

/// 每家 provider 最近一次登录失败时间（内存态；单实例部署足够，
/// 重启后最多多撞一次锁）。
fn login_failures() -> &'static Mutex<HashMap<i32, DateTime<Utc>>> {
    static FAILURES: std::sync::OnceLock<Mutex<HashMap<i32, DateTime<Utc>>>> =
        std::sync::OnceLock::new();
    FAILURES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 清空登录失败冷却记录（测试隔离用：模块级静态在集成测试间共享，
/// 失败用例会给后续用例留下冷却状态）。
pub fn reset_login_failures() {
    login_failures().lock().unwrap().clear();
}

/// 统一用量入口（由 `query_provider_usage` 对 SenseNova host 调用）：
/// refresh_token 有效 → 续期查询；缺失或明确鉴权失败 → 登录一次 → 写回新 refresh_token →
/// 用新 token 重试一次续期查询。
///
/// `login` 是带 cookie/重定向的登录客户端（仅当需要登录时才真正发请求）。
pub async fn fetch_sensenova(
    http: &UsageHttp,
    login: &SensenovaLogin,
    creds: &Credentials<'_>,
    ctx: &SensenovaContext<'_>,
) -> Result<FetchOutput, UsageError> {
    // 有 refresh_token → 先走续期查询。
    if let Some(refresh_token) = creds.extra_str("refresh_token") {
        match renew_and_query(http, refresh_token).await {
            Ok((output, rotated)) => {
                // 轮换出的新 token 立即写回（与旧值相同/缺失则跳过）。
                if let Some(new_token) = rotated {
                    super::super::write_back_extra_key(
                        ctx.db,
                        ctx.provider_id,
                        "refresh_token",
                        &new_token,
                    )
                    .await?;
                }
                return Ok(output);
            }
            Err(UsageError::Auth) => {
                // 明确失效：有账号密码 → 走登录重试；没有 → 保持原「凭据失效」语义
                // （不能自愈，避免报误导性的「缺少 username」）。
                if creds.extra_str("username").is_none() && creds.extra_str("password").is_none() {
                    return Err(UsageError::Auth);
                }
            }
            Err(e) => return Err(e), // 网络/5xx/解析失败不登录
        }
    }
    // 重新登录最多一次（refresh_token 缺失时同样先进来）。
    let username = creds.require("username")?;
    let password = creds.require("password")?;

    // 冷却期内不重试登录（账号已因连续失败被商汤临时锁定，再试会刷新锁定窗口）。
    if let Some(failed_at) = login_failures()
        .lock()
        .unwrap()
        .get(&ctx.provider_id)
        .copied()
        && Utc::now().signed_duration_since(failed_at)
            < chrono::Duration::from_std(LOGIN_COOLDOWN).unwrap()
    {
        return Err(UsageError::Upstream(429, LOGIN_COOLDOWN_MSG.to_string()));
    }

    let tokens = login.login(username, password).await.map_err(|e| {
        // 凭据错 / 账号锁定才冷却（避免反复撞锁）；网络抖动不冷却，下次仍可重试。
        if matches!(e, UsageError::Auth | UsageError::Upstream(_, _)) {
            login_failures()
                .lock()
                .unwrap()
                .insert(ctx.provider_id, Utc::now());
        }
        e
    })?;
    // 登录成功 → 清除冷却记录。
    login_failures().lock().unwrap().remove(&ctx.provider_id);
    // 新 refresh_token 必须先回写（重读最新行、严格解密、只合并该键），失败不继续重试。
    super::super::write_back_extra_key(
        ctx.db,
        ctx.provider_id,
        "refresh_token",
        &tokens.refresh_token,
    )
    .await?;
    let (output, _) = renew_and_query(http, &tokens.refresh_token).await?;
    Ok(output)
}

/// 用 refresh_token 续期并查询 pool-usage。
/// 返回 (用量数据, 轮换出的新 refresh_token；与旧值相同或缺失时为 None)。
///
/// 鉴权失败（HTTP 401/403、HTTP 400 + `invalid_grant`，或 200 但续期响应
/// `{"error":"invalid_grant"}` / 缺 access_token）→ `UsageError::Auth`（触发调用方登录）。
async fn renew_and_query(
    http: &UsageHttp,
    refresh_token: &str,
) -> Result<(FetchOutput, Option<String>), UsageError> {
    // refresh_token 为 JWT/base64url 字符，均为 form 安全字符，直接内嵌。
    let form = format!("grant_type=refresh_token&client_id=nova&refresh_token={refresh_token}");
    let reply = http.post_form(TOKEN_URL, &[], &form).await?;
    ensure_not_auth_error(&reply)?;
    // 商汤 OAuth token 端点对失效 refresh_token 返回 HTTP 400 + `invalid_grant`
    // （非 401/403），同样视为明确鉴权失败以触发登录自愈。
    if reply.status == 400 && reply.body.to_lowercase().contains("\"invalid_grant\"") {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    let token: Value = serde_json::from_str(&reply.body)
        .map_err(|e| UsageError::Parse(format!("续期响应不是合法 JSON：{e}")))?;
    // invalid_grant 等错误响应（200 但无 access_token）→ 视为鉴权失败。
    let access_token = token
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(UsageError::Auth)?;
    let rotated = token
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != refresh_token)
        .map(str::to_string);

    let reply = http
        .get(
            USAGE_URL,
            &[
                ("Authorization", format!("Bearer {access_token}")),
                ("Accept-Language", "zh-CN".to_string()),
            ],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_pool_usage(&reply.body).map(|output| (output, rotated))
}

fn parse_pool_usage(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let plan = v
        .get("plan")
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut windows = Vec::new();
    let pools = v.get("pools").and_then(Value::as_array);
    for pool in pools.into_iter().flatten() {
        let label = pool.get("name").and_then(Value::as_str).map(str::to_string);
        for (key, kind) in [
            ("window_5h", WindowKind::FiveHour),
            ("window_7d", WindowKind::Weekly),
        ] {
            if let Some(w) = pool.get(key).and_then(|d| window_from(kind, d)) {
                windows.push(QuotaWindow {
                    label: label.clone(),
                    ..w
                });
            }
        }
    }
    Ok(FetchOutput::Quota { plan, windows })
}

/// 单池窗口：limit/used/remaining 均为字符串数字，reset_at 为秒级时间戳字符串。
fn window_from(kind: WindowKind, detail: &Value) -> Option<QuotaWindow> {
    let limit = num(detail.get("limit")?)?;
    let used = num(detail.get("used")?)?;
    let resets_at = detail.get("reset_at").and_then(reset_secs);
    Some(QuotaWindow::from_used_limit(
        kind,
        used,
        limit,
        resets_at,
        Some("积分"),
    ))
}

/// 秒级时间戳（字符串或数字）→ UTC。
fn reset_secs(v: &Value) -> Option<DateTime<Utc>> {
    let n = match v {
        Value::String(s) => s.trim().parse::<i64>().ok()?,
        Value::Number(n) => n.as_i64()?,
        _ => return None,
    };
    ts_secs(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_pool_parses_to_labeled_windows() {
        let body = r#"{
          "plan": { "id": "free", "name": "Free Plan", "type": "TOKEN_PLAN_PLAN_TYPE_FREE" },
          "pools": [
            { "id": "pool_a", "name": "通用积分池", "pool_type": "default",
              "window_5h": { "limit": "60000", "used": "33586.30032", "remaining": "26413.69968", "reset_at": "1788365437" },
              "window_7d": { "limit": "600000", "used": "51388.65712", "remaining": "548611.34288", "reset_at": "1788862237" } },
            { "id": "pool_b", "name": "Flash-Lite积分池", "pool_type": "dedicated",
              "window_5h": { "limit": "10000", "used": "9999", "remaining": "1", "reset_at": "1788365437" } }
          ]
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_pool_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("Free Plan"));
        assert_eq!(windows.len(), 3);
        // 每池独立产出，label = 池名。
        assert_eq!(windows[0].window, WindowKind::FiveHour);
        assert_eq!(windows[0].label.as_deref(), Some("通用积分池"));
        assert_eq!(windows[0].used, Some(33586.3));
        assert_eq!(windows[0].limit, Some(60000.0));
        assert_eq!(windows[0].unit.as_deref(), Some("积分"));
        assert_eq!(windows[1].window, WindowKind::Weekly);
        assert_eq!(windows[1].label.as_deref(), Some("通用积分池"));
        assert_eq!(windows[2].window, WindowKind::FiveHour);
        assert_eq!(windows[2].label.as_deref(), Some("Flash-Lite积分池"));
        assert_eq!(windows[2].remaining_percent_value(), Some(0.01));
    }

    #[test]
    fn reset_at_seconds_string_becomes_utc() {
        let body = r#"{ "pools": [ { "name": "通用积分池",
          "window_5h": { "limit": "60000", "used": "0", "remaining": "60000", "reset_at": "1788365437" } } ] }"#;
        let FetchOutput::Quota { windows, .. } = parse_pool_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(
            windows[0].resets_at.unwrap(),
            chrono::DateTime::parse_from_rfc3339("2026-09-02T16:10:37Z").unwrap()
        );
    }

    #[test]
    fn pool_without_windows_is_skipped() {
        let body = r#"{ "pools": [
            { "name": "空池" },
            { "name": "残缺池", "window_5h": { "limit": "100" } }
        ] }"#;
        let FetchOutput::Quota { windows, .. } = parse_pool_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows.is_empty());
    }

    #[test]
    fn invalid_json_is_parse_error() {
        assert!(parse_pool_usage("not json").is_err());
    }
}
