//! 飞书通知渠道（ADR-0023）。
//!
//! 自研 HTTP 客户端直调飞书官方端点，不引入 `oapi-sdk-rust`（其默认 features
//! 编译不过、未发布 crates.io）。三个端点族：
//!
//! - Open API（取 tenant_access_token、发消息）：`https://open.feishu.cn`
//! - 账号域（扫码一键创建应用）：`https://accounts.feishu.cn`
//!
//! 只支持飞书国内版，不做 Lark 国际版的域名切换。

pub mod feishu;
pub mod notify;
pub mod register;

use anyhow::anyhow;
use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, EntityTrait, Set};

use crate::crypto;
use crate::entity::notification_channel;

/// 渠道标识：当前只支持飞书。
pub const FEISHU_CHANNEL: &str = "feishu";

/// 测试发送的消息文案。
pub const TEST_MESSAGE: &str = "【llm-gateway】这是一条测试消息，飞书通知配置可用。";

/// 接收者类型：飞书 `receive_id_type` 的取值子集。
pub const RECEIVER_TYPE_OPEN_ID: &str = "open_id";
pub const RECEIVER_TYPE_EMAIL: &str = "email";

/// 渠道配置明文（整段加密后落 `notification_channel.config`）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default = "default_receiver_type")]
    pub receiver_type: String,
    pub receiver: String,
}

fn default_receiver_type() -> String {
    RECEIVER_TYPE_OPEN_ID.to_string()
}

impl FeishuConfig {
    /// 值级校验，返回中文错误消息。
    pub fn validate(&self) -> Result<(), String> {
        if self.app_id.trim().is_empty() {
            return Err("App ID 不能为空".to_string());
        }
        if self.receiver_type != RECEIVER_TYPE_OPEN_ID && self.receiver_type != RECEIVER_TYPE_EMAIL
        {
            return Err("接收者类型只能是 open_id 或 email".to_string());
        }
        if self.receiver.trim().is_empty() {
            return Err("接收者不能为空".to_string());
        }
        Ok(())
    }
}

/// 解码存储的密文配置；解不开（密钥轮换/损坏）或非法 JSON 时返回 None
/// （按「未配置」处理，`crypto` 侧已打 warn）。
fn decode_config(stored: &str) -> Option<FeishuConfig> {
    serde_json::from_str(&crypto::decrypt_or_passthrough(stored)).ok()
}

/// `GET /api/notification/feishu` 的响应体。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelResponse {
    pub configured: bool,
    pub app_id: String,
    pub app_secret_masked: String,
    pub receiver_type: String,
    pub receiver: String,
    pub enable: bool,
    pub last_error: String,
    pub last_sent_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ChannelResponse {
    fn unconfigured() -> Self {
        Self {
            configured: false,
            app_id: String::new(),
            app_secret_masked: String::new(),
            receiver_type: RECEIVER_TYPE_OPEN_ID.to_string(),
            receiver: String::new(),
            enable: true,
            last_error: String::new(),
            last_sent_at: None,
        }
    }
}

/// 读取通知渠道行（单行表，按渠道常量取）。
async fn load_row(db: &DatabaseConnection) -> Result<Option<notification_channel::Model>, DbErr> {
    notification_channel::Entity::find_by_id(FEISHU_CHANNEL)
        .one(db)
        .await
}

/// 读取渠道配置用于展示（appSecret 掩码）。未配置不是错误，返回
/// `configured:false` 的空壳。
pub async fn read_channel(db: &DatabaseConnection) -> Result<ChannelResponse, DbErr> {
    let Some(row) = load_row(db).await? else {
        return Ok(ChannelResponse::unconfigured());
    };

    let Some(config) = decode_config(&row.config) else {
        return Ok(ChannelResponse::unconfigured());
    };

    Ok(ChannelResponse {
        configured: true,
        app_id: config.app_id,
        app_secret_masked: crypto::mask(&config.app_secret),
        receiver_type: config.receiver_type,
        receiver: config.receiver,
        enable: row.enable,
        last_error: row.last_error,
        last_sent_at: row.last_sent_at,
    })
}

/// 保存渠道配置。`app_secret` 为 `None`/空串时沿用库中原值（编辑时不必重输
/// 凭据）；首次创建没有原值可用，必须提供。
pub async fn save_channel(
    db: &DatabaseConnection,
    app_id: String,
    app_secret: Option<String>,
    receiver_type: String,
    receiver: String,
    enable: bool,
) -> Result<(), String> {
    let existing = load_row(db)
        .await
        .map_err(|e| format!("读取通知配置失败：{e}"))?;
    let stored_secret = existing
        .as_ref()
        .and_then(|row| decode_config(&row.config))
        .map(|c| c.app_secret)
        .unwrap_or_default();

    let app_secret = match app_secret.filter(|s| !s.trim().is_empty()) {
        Some(secret) => secret,
        None if !stored_secret.is_empty() => stored_secret,
        None => return Err("App Secret 不能为空".to_string()),
    };

    let config = FeishuConfig {
        app_id,
        app_secret,
        receiver_type,
        receiver,
    };
    config.validate()?;

    let encoded = crypto::encrypt(
        &serde_json::to_string(&config).map_err(|e| format!("序列化通知配置失败：{e}"))?,
    );
    let now = chrono::Utc::now();

    match existing {
        Some(row) => {
            let mut active: notification_channel::ActiveModel = row.into();
            active.config = Set(encoded);
            active.enable = Set(enable);
            active.updated_at = Set(now);
            active
                .update(db)
                .await
                .map_err(|e| format!("保存通知配置失败：{e}"))?;
        }
        None => {
            notification_channel::ActiveModel {
                channel: Set(FEISHU_CHANNEL.to_string()),
                config: Set(encoded),
                enable: Set(enable),
                last_error: Set(String::new()),
                last_sent_at: Set(None),
                updated_at: Set(now),
            }
            .insert(db)
            .await
            .map_err(|e| format!("保存通知配置失败：{e}"))?;
        }
    }
    Ok(())
}

/// 读取渠道配置明文；未配置/解不开返回 None。供发送路径使用（不经掩码）。
pub async fn load_config(db: &DatabaseConnection) -> Result<Option<FeishuConfig>, anyhow::Error> {
    let row = load_row(db)
        .await
        .map_err(|e| anyhow!("读取通知配置失败：{e}"))?;
    Ok(row.and_then(|row| decode_config(&row.config)))
}

/// 读取「已配置且已启用」的渠道配置；未配置或已停用返回 None。供自动通知使用。
pub async fn load_active_config(
    db: &DatabaseConnection,
) -> Result<Option<FeishuConfig>, anyhow::Error> {
    let row = load_row(db)
        .await
        .map_err(|e| anyhow!("读取通知配置失败：{e}"))?;
    Ok(row
        .filter(|row| row.enable)
        .and_then(|row| decode_config(&row.config)))
}

/// 回写最近一次发送结果（成功清空 `last_error`）。失败只打日志——回写本身
/// 失败不该再掩盖原始发送结果。
pub async fn record_send_result(db: &DatabaseConnection, error: Option<&str>) {
    let Some(row) = load_row(db).await.ok().flatten() else {
        return;
    };

    let mut active: notification_channel::ActiveModel = row.into();
    active.last_error = Set(error.unwrap_or_default().to_string());
    active.last_sent_at = Set(Some(chrono::Utc::now()));
    if let Err(e) = active.update(db).await {
        tracing::warn!("回写通知发送结果失败：{e}");
    }
}

/// 用给定值发一条测试消息（**不落库配置**，支持保存前先验证）。
/// `app_secret` 缺省/空串时回落库中已存值（刷新页面后可直接测试）。
pub async fn send_test(
    db: &DatabaseConnection,
    app_id: String,
    app_secret: Option<String>,
    receiver_type: String,
    receiver: String,
) -> Result<(), String> {
    let stored = load_config(db).await.map_err(|e| e.to_string())?;
    let app_secret = match app_secret.filter(|s| !s.trim().is_empty()) {
        Some(secret) => secret,
        None => stored.map(|c| c.app_secret).unwrap_or_default(),
    };

    let config = FeishuConfig {
        app_id,
        app_secret,
        receiver_type,
        receiver,
    };
    config.validate()?;
    if config.app_secret.trim().is_empty() {
        return Err("App Secret 不能为空".to_string());
    }

    let result = feishu::send(&config, TEST_MESSAGE).await;
    record_send_result(db, result.as_ref().err().map(String::as_str)).await;
    result
}
