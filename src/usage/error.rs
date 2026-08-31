//! 用量查询的错误类型与 HTTP 状态映射。

use crate::i18n::Lang;

/// 用量查询失败原因。HTTP 映射见 `crate::routes::providers::get_provider_usage`：
/// 前四类为 400（用户可修正），后三类为 502（上游/网络问题）。
#[derive(Debug, thiserror::Error)]
pub enum UsageError {
    #[error("该供应商未开启用量查询")]
    NotEnabled,
    #[error("暂不支持该供应商的用量查询")]
    Unsupported,
    #[error("缺少用量查询所需凭据：{0}")]
    MissingCredential(String),
    #[error("用量查询凭据无效或已过期")]
    Auth,
    #[error("上游接口返回错误（HTTP {0}）：{1}")]
    Upstream(u16, String),
    #[error("网络请求失败：{0}")]
    Network(String),
    #[error("上游响应解析失败：{0}")]
    Parse(String),
}

impl UsageError {
    /// 是否为 400 类（用户输入/凭据问题）；否则按 502 上游错误处理。
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            UsageError::NotEnabled
                | UsageError::Unsupported
                | UsageError::MissingCredential(_)
                | UsageError::Auth
        )
    }

    /// 按管理后台语言生成用户可见消息（默认 zh 输出与 Display 一致）。
    pub fn user_message(&self, lang: Lang) -> String {
        match self {
            UsageError::NotEnabled => lang
                .tr(
                    "该供应商未开启用量查询",
                    "usage query is not enabled for this provider",
                )
                .to_string(),
            UsageError::Unsupported => lang
                .tr(
                    "暂不支持该供应商的用量查询",
                    "usage query is not supported for this provider",
                )
                .to_string(),
            UsageError::MissingCredential(field) => {
                if lang == Lang::En {
                    format!("missing credential required for usage query: {field}")
                } else {
                    format!("缺少用量查询所需凭据：{field}")
                }
            }
            UsageError::Auth => lang
                .tr(
                    "用量查询凭据无效或已过期",
                    "usage query credentials are invalid or expired",
                )
                .to_string(),
            UsageError::Upstream(status, detail) => {
                if lang == Lang::En {
                    format!("upstream returned an error (HTTP {status}): {detail}")
                } else {
                    format!("上游接口返回错误（HTTP {status}）：{detail}")
                }
            }
            UsageError::Network(detail) => {
                if lang == Lang::En {
                    format!("network request failed: {detail}")
                } else {
                    format!("网络请求失败：{detail}")
                }
            }
            UsageError::Parse(detail) => {
                if lang == Lang::En {
                    format!("failed to parse upstream response: {detail}")
                } else {
                    format!("上游响应解析失败：{detail}")
                }
            }
        }
    }
}
