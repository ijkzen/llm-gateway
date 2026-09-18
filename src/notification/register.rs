//! 飞书扫码一键创建应用（OAuth 2.0 Device Authorization Grant，ADR-0023）。
//!
//! 流程：`action=begin` 拿 `device_code` 与二维码内容 → 管理员用飞书客户端扫码
//! 确认 → `action=poll` 轮询到 `client_id`/`client_secret`。
//!
//! 只支持飞书国内版，不处理 `tenant_brand=lark` 的域名切换。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::feishu;

/// 注册端点路径（挂在账号域下）。
const REGISTRATION_PATH: &str = "/oauth/v1/app/registration";

/// 飞书未返回有效期时的兜底（官方 SDK 默认 600 秒）。
const DEFAULT_EXPIRES_IN: u64 = 600;

/// 飞书未返回轮询间隔时的兜底。
const DEFAULT_INTERVAL_SECS: u64 = 5;

/// `action=poll` 的落点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    Pending,
    SlowDown,
    Success {
        app_id: String,
        app_secret: String,
        receiver: String,
    },
    Denied,
    Expired,
}

/// 解析 poll 响应体。
///
/// **必须解析 HTTP 非 2xx 的 body**：`authorization_pending` / `slow_down` /
/// `access_denied` / `expired_token` 全部以 400 返回，把非 2xx 当硬错误会让
/// 第一次轮询就杀掉整个会话。空 `error` 且无凭据同样视为继续轮询。
pub fn parse_poll_body(body: &Value) -> PollOutcome {
    if let (Some(app_id), Some(app_secret)) = (
        body.get("client_id").and_then(Value::as_str),
        body.get("client_secret").and_then(Value::as_str),
    ) {
        let receiver = body
            .get("user_info")
            .and_then(|info| info.get("open_id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        return PollOutcome::Success {
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            receiver: receiver.to_string(),
        };
    }

    match body.get("error").and_then(Value::as_str) {
        Some("slow_down") => PollOutcome::SlowDown,
        Some("access_denied") => PollOutcome::Denied,
        Some("expired_token") => PollOutcome::Expired,
        _ => PollOutcome::Pending,
    }
}

/// 从 begin 响应体取会话有效期：`expires_in` 与 `expire_in` 两种字段名都接受
/// （不同 SDK 版本口径不一）。
fn parse_expires_in(body: &Value) -> u64 {
    body.get("expires_in")
        .or_else(|| body.get("expire_in"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_EXPIRES_IN)
}

/// `action=begin` 的结果。
pub struct BeginResult {
    pub qr_url: String,
    pub device_code: String,
    pub expires_in: u64,
    pub interval_secs: u64,
}

/// 发起注册会话。
pub async fn begin_request() -> Result<BeginResult, String> {
    let url = format!("{}{REGISTRATION_PATH}", feishu::accounts_base());
    let (status, body) = feishu::post_form(
        &url,
        &[
            ("action", "begin"),
            ("archetype", "PersonalAgent"),
            ("auth_method", "client_secret"),
            ("request_user_info", "open_id"),
        ],
    )
    .await?;

    let field = |name: &str| {
        body.get(name)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
    };
    // 二维码内容 = verification_uri_complete 原样（已含 user_code，不要自己拼）。
    let qr_url = field("verification_uri_complete")
        .ok_or_else(|| format!("飞书未返回 verification_uri_complete（HTTP {status}）"))?;
    let device_code =
        field("device_code").ok_or_else(|| format!("飞书未返回 device_code（HTTP {status}）"))?;

    Ok(BeginResult {
        qr_url: qr_url.to_string(),
        device_code: device_code.to_string(),
        expires_in: parse_expires_in(&body),
        interval_secs: body
            .get("interval")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_INTERVAL_SECS)
            .max(1),
    })
}

/// 向飞书轮询一次注册状态。
pub async fn poll_request(device_code: &str) -> Result<PollOutcome, String> {
    let url = format!("{}{REGISTRATION_PATH}", feishu::accounts_base());
    let (_status, body) =
        feishu::post_form(&url, &[("action", "poll"), ("device_code", device_code)]).await?;
    Ok(parse_poll_body(&body))
}

/// 前端可见的注册状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationStatus {
    Pending,
    SlowDown,
    Success,
    Denied,
    Expired,
}

impl RegistrationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::SlowDown => "slow_down",
            Self::Success => "success",
            Self::Denied => "denied",
            Self::Expired => "expired",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Success | Self::Denied | Self::Expired)
    }
}

/// 扫码创建返回的凭据：回填前端表单，由用户确认后再保存。
#[derive(Debug, Clone)]
pub struct Credentials {
    pub app_id: String,
    pub app_secret: String,
    pub receiver: String,
}

struct Session {
    id: String,
    device_code: String,
    expires_in: u64,
    interval_secs: u64,
    started_at: Instant,
    last_poll_at: Option<Instant>,
    status: RegistrationStatus,
    credentials: Option<Credentials>,
}

impl Session {
    fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= Duration::from_secs(self.expires_in)
    }

    /// 距上次真实轮询是否已过 `interval`（前端 2-3 秒的节奏不会打爆飞书侧）。
    fn due_for_poll(&self) -> bool {
        match self.last_poll_at {
            Some(at) => at.elapsed() >= Duration::from_secs(self.interval_secs),
            None => true,
        }
    }

    fn apply(&mut self, outcome: PollOutcome) {
        match outcome {
            PollOutcome::Pending => self.status = RegistrationStatus::Pending,
            PollOutcome::SlowDown => {
                // 飞书要求退避：轮询间隔 +5 秒。
                self.interval_secs += 5;
                self.status = RegistrationStatus::SlowDown;
            }
            PollOutcome::Denied => self.status = RegistrationStatus::Denied,
            PollOutcome::Expired => self.status = RegistrationStatus::Expired,
            PollOutcome::Success {
                app_id,
                app_secret,
                receiver,
            } => {
                self.status = RegistrationStatus::Success;
                self.credentials = Some(Credentials {
                    app_id,
                    app_secret,
                    receiver,
                });
            }
        }
    }

    fn response(&self) -> StatusResponse {
        let credentials = self.credentials.as_ref();
        StatusResponse {
            status: self.status.as_str(),
            app_id: credentials.map(|c| c.app_id.clone()),
            app_secret: credentials.map(|c| c.app_secret.clone()),
            receiver: credentials.map(|c| c.receiver.clone()),
            receiver_type: credentials.map(|_| super::RECEIVER_TYPE_OPEN_ID),
        }
    }
}

/// `POST /api/notification/feishu/register` 的 data。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginResponse {
    pub session_id: String,
    pub qr_url: String,
    pub expires_in: u64,
}

/// `GET /api/notification/feishu/register/{id}` 的 data。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_type: Option<&'static str>,
}

impl StatusResponse {
    fn expired() -> Self {
        Self {
            status: RegistrationStatus::Expired.as_str(),
            app_id: None,
            app_secret: None,
            receiver: None,
            receiver_type: None,
        }
    }
}

/// 注册会话槽。**单槽位**：同一时刻只允许一个进行中的扫码会话，新 `begin`
/// 直接替换旧的（管理员只有一个人，不需要多会话管理）。
#[derive(Clone, Default)]
pub struct RegistrationSlot(Arc<Mutex<Option<Session>>>);

impl RegistrationSlot {
    /// 发起一次注册会话，替换旧会话。
    pub async fn begin(&self) -> Result<BeginResponse, String> {
        let begun = begin_request().await?;
        let session_id = uuid::Uuid::new_v4().to_string();

        let mut guard = self.0.lock().unwrap();
        *guard = Some(Session {
            id: session_id.clone(),
            device_code: begun.device_code,
            expires_in: begun.expires_in,
            interval_secs: begun.interval_secs,
            started_at: Instant::now(),
            last_poll_at: None,
            status: RegistrationStatus::Pending,
            credentials: None,
        });

        Ok(BeginResponse {
            session_id,
            qr_url: begun.qr_url,
            expires_in: begun.expires_in,
        })
    }

    /// 查询注册状态。对飞书的轮询受 `interval` 节流；终态与超时直接回缓存。
    pub async fn poll(&self, session_id: &str) -> Result<StatusResponse, String> {
        // 先在锁内决定「回缓存还是真轮询」，再出锁做 HTTP（不跨 await 持锁）。
        let device_code = {
            let mut guard = self.0.lock().unwrap();
            let Some(session) = guard.as_mut() else {
                return Ok(StatusResponse::expired());
            };
            if session.id != session_id {
                return Ok(StatusResponse::expired());
            }
            if session.is_expired() {
                session.status = RegistrationStatus::Expired;
                session.credentials = None;
                return Ok(session.response());
            }
            if session.status.is_terminal() || !session.due_for_poll() {
                return Ok(session.response());
            }
            // 先记账再请求：请求失败也不该在窗口内重试打爆上游。
            session.last_poll_at = Some(Instant::now());
            session.device_code.clone()
        };

        let outcome = poll_request(&device_code).await?;

        let mut guard = self.0.lock().unwrap();
        let Some(session) = guard.as_mut() else {
            return Ok(StatusResponse::expired());
        };
        if session.id != session_id {
            return Ok(StatusResponse::expired());
        }
        session.apply(outcome);
        Ok(session.response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// poll 响应体各落点：成功取顶层 client_id/client_secret 与 user_info.open_id。
    #[test]
    fn parse_poll_body_covers_all_outcomes() {
        assert_eq!(
            parse_poll_body(&json!({"error": "authorization_pending"})),
            PollOutcome::Pending
        );
        assert_eq!(
            parse_poll_body(&json!({"error": "slow_down"})),
            PollOutcome::SlowDown
        );
        assert_eq!(
            parse_poll_body(&json!({"error": "access_denied"})),
            PollOutcome::Denied
        );
        assert_eq!(
            parse_poll_body(&json!({"error": "expired_token"})),
            PollOutcome::Expired
        );
        // 空 error 且无凭据：继续轮询。
        assert_eq!(parse_poll_body(&json!({})), PollOutcome::Pending);

        assert_eq!(
            parse_poll_body(&json!({
                "client_id": "cli_x",
                "client_secret": "sec_y",
                "user_info": {"open_id": "ou_z", "tenant_brand": "feishu"},
            })),
            PollOutcome::Success {
                app_id: "cli_x".to_string(),
                app_secret: "sec_y".to_string(),
                receiver: "ou_z".to_string(),
            }
        );
    }

    /// 有 client_id 但缺 client_secret 不算成功（避免拿半截凭据去保存）。
    #[test]
    fn parse_poll_body_requires_both_credentials() {
        assert_eq!(
            parse_poll_body(&json!({"client_id": "cli_x"})),
            PollOutcome::Pending
        );
    }

    /// `expires_in` 与 `expire_in` 两种字段名都接受，缺省 600。
    #[test]
    fn parse_expires_in_accepts_both_field_names() {
        assert_eq!(parse_expires_in(&json!({"expires_in": 3600})), 3600);
        assert_eq!(parse_expires_in(&json!({"expire_in": 120})), 120);
        assert_eq!(parse_expires_in(&json!({})), DEFAULT_EXPIRES_IN);
    }

    fn session() -> Session {
        Session {
            id: "s-1".to_string(),
            device_code: "dc-1".to_string(),
            expires_in: 600,
            interval_secs: 5,
            started_at: Instant::now(),
            last_poll_at: None,
            status: RegistrationStatus::Pending,
            credentials: None,
        }
    }

    /// slow_down 必须退避：间隔 +5 秒，且状态透出给前端。
    #[test]
    fn slow_down_backs_off_by_five_seconds() {
        let mut session = session();
        session.apply(PollOutcome::SlowDown);
        assert_eq!(session.interval_secs, 10);
        assert_eq!(session.status, RegistrationStatus::SlowDown);
    }

    /// 成功即终态，凭据随响应回给前端（receiverType 恒 open_id）。
    #[test]
    fn success_is_terminal_and_carries_credentials() {
        let mut session = session();
        session.apply(PollOutcome::Success {
            app_id: "cli_new".to_string(),
            app_secret: "sec_new".to_string(),
            receiver: "ou_scanner".to_string(),
        });

        assert!(session.status.is_terminal());
        let response = session.response();
        assert_eq!(response.status, "success");
        assert_eq!(response.app_id.as_deref(), Some("cli_new"));
        assert_eq!(response.app_secret.as_deref(), Some("sec_new"));
        assert_eq!(response.receiver.as_deref(), Some("ou_scanner"));
        assert_eq!(response.receiver_type, Some("open_id"));
    }

    /// 首次轮询不受节流；刚轮询过则窗口内不再打飞书。
    #[test]
    fn first_poll_is_immediate_then_throttled() {
        let mut session = session();
        assert!(session.due_for_poll(), "首次轮询应放行");
        session.last_poll_at = Some(Instant::now());
        assert!(!session.due_for_poll(), "interval 内应被节流");
    }
}
