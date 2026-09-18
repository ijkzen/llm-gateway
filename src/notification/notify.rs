//! 自动转换通知：供应商可用性发生**真实**状态转换时异步发飞书消息（ADR-0023）。
//!
//! 触发点收敛在 `crate::availability` 的四个转换动作内部（确有转换的分支），
//! 未来新增调用方不会漏发；管理员手动启停走 `*_manual`，不经这里。

use chrono::Utc;
use sea_orm::{DatabaseConnection, EntityTrait};

use super::feishu;
use crate::entity::provider;

/// 触发通知的自动转换类型。原因文案由枚举自带，调用方只传上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// 额度/余额耗尽自动停用（含订阅制边界实测失败）。`label` = 订阅额度 / 余额。
    QuotaDisabled { label: String },
    /// 额度/余额恢复自动启用（含边界实测成功）。
    QuotaRecovered { label: String },
    /// 连续失败达到阈值熔断停用。
    FailureDisabled { consecutive: u32 },
    /// 自动恢复探测成功启用。
    FailureRecovered,
}

impl Transition {
    /// 消息标题。
    fn headline(&self) -> &'static str {
        match self {
            Self::QuotaDisabled { .. } | Self::FailureDisabled { .. } => "【供应商自动停用】",
            Self::QuotaRecovered { .. } | Self::FailureRecovered => "【供应商自动启用】",
        }
    }

    /// 原因行。
    fn reason(&self) -> String {
        match self {
            Self::QuotaDisabled { label } => format!("{label}耗尽"),
            Self::QuotaRecovered { label } => format!("{label}恢复"),
            Self::FailureDisabled { consecutive } => format!("连续失败 {consecutive} 次"),
            Self::FailureRecovered => "自动恢复探测成功".to_string(),
        }
    }
}

/// 渲染消息正文（纯文本多行）。时间按设置表时区展示，与 cron/统计口径一致。
pub fn render(
    transition: &Transition,
    provider_name: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let local = now.with_timezone(&crate::app_settings::timezone_sync());
    format!(
        "{}\n供应商：{}\n原因：{}\n时间：{}",
        transition.headline(),
        provider_name,
        transition.reason(),
        local.format("%Y-%m-%d %H:%M:%S"),
    )
}

/// 异步发送入口：**同步函数**，内部 `tokio::spawn`。
///
/// 熔断（`disable_for_failures`）发生在转发**热路径**上，绝不能因飞书 HTTP
/// 调用而阻塞，所以这里不返回 future 让调用方 await。
pub fn spawn_provider_transition(
    db: &DatabaseConnection,
    provider_id: i32,
    transition: Transition,
) {
    let db = db.clone();
    tokio::spawn(async move {
        if let Err(e) = notify_provider_transition(&db, provider_id, transition).await {
            tracing::warn!(provider_id, "发送供应商转换通知失败：{e}");
        }
    });
}

/// 通知主体（`await` 直调，便于确定性测试）。
///
/// 未配置 / 已停用 / 供应商已删除一律静默返回（不构成错误）。发送结果无论
/// 成败都回写 `last_error` / `last_sent_at`。
pub async fn notify_provider_transition(
    db: &DatabaseConnection,
    provider_id: i32,
    transition: Transition,
) -> Result<(), String> {
    let Some(config) = super::load_active_config(db)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };

    let Some(row) = provider::Entity::find_by_id(provider_id)
        .one(db)
        .await
        .map_err(|e| format!("读取供应商失败：{e}"))?
    else {
        return Ok(());
    };

    let text = render(&transition, &row.name, Utc::now());
    let result = feishu::send(&config, &text).await;
    super::record_send_result(db, result.as_ref().err().map(String::as_str)).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 9, 18, 3, 30, 0).unwrap()
    }

    /// 四类转换的标题与原因行。时间按设置表时区（默认 Asia/Shanghai = UTC+8）渲染。
    #[test]
    fn renders_each_transition_kind() {
        let cases = [
            (
                Transition::QuotaDisabled {
                    label: "订阅额度".to_string(),
                },
                "【供应商自动停用】",
                "订阅额度耗尽",
            ),
            (
                Transition::QuotaRecovered {
                    label: "余额".to_string(),
                },
                "【供应商自动启用】",
                "余额恢复",
            ),
            (
                Transition::FailureDisabled { consecutive: 5 },
                "【供应商自动停用】",
                "连续失败 5 次",
            ),
            (
                Transition::FailureRecovered,
                "【供应商自动启用】",
                "自动恢复探测成功",
            ),
        ];

        for (transition, headline, reason) in cases {
            let text = render(&transition, "某供应商", at());
            assert!(text.starts_with(headline), "标题不符：{text}");
            assert!(text.contains("供应商：某供应商"), "缺供应商名：{text}");
            assert!(
                text.contains(&format!("原因：{reason}")),
                "原因不符：{text}"
            );
            assert!(
                text.contains("时间：2026-09-18 11:30:00"),
                "时间应按 Asia/Shanghai 渲染：{text}"
            );
        }
    }
}
