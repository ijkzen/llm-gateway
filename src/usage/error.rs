//! 用量查询的错误类型与 HTTP 状态映射。

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
}
