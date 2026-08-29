//! 刷新：按协议调用供应商的 Models 接口，拉取远端模型 ID 列表。

use std::time::Duration;

use serde::Deserialize;

/// 协议类型取值（与 `entity::provider` 的注释约定一致）。
pub const PROTOCOL_OPENAI_COMPATIBLE: i32 = 0;
pub const PROTOCOL_OPENAI_RESPONSE: i32 = 1;
pub const PROTOCOL_ANTHROPIC: i32 = 2;
pub const PROTOCOL_GEMINI: i32 = 3;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// 远端错误消息透传给前端时的截断长度。
const ERROR_BODY_LIMIT: usize = 200;

/// 拼接 Models 接口 URL：去掉尾部 `/` 后，若末段不是版本段则按协议补默认版本段。
///
/// 种子模板的 base_url 普遍已带 `/v1`（Anthropic 兼容端点亦然），
/// Gemini 官方根地址则不带版本段。
pub fn build_models_url(base_url: &str, protocol_type: i32) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("");
    let default_version = if protocol_type == PROTOCOL_GEMINI {
        "v1beta"
    } else {
        "v1"
    };
    if matches!(last, "v1" | "v1beta" | "v1alpha") {
        format!("{trimmed}/models")
    } else {
        format!("{trimmed}/{default_version}/models")
    }
}

/// 拉取远端模型 ID 列表。
///
/// `api_key` 为调用方解密后的明文；`custom_header` 为 JSON 对象字符串，
/// 展开为额外请求头。错误消息为中文、可直接展示给用户。
pub async fn fetch_remote_model_ids(
    base_url: &str,
    protocol_type: i32,
    api_key: &str,
    custom_header: &str,
) -> Result<Vec<String>, String> {
    let url = build_models_url(base_url, protocol_type);
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败：{e}"))?;

    let mut request = client.get(&url);
    request = match protocol_type {
        PROTOCOL_ANTHROPIC => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        PROTOCOL_GEMINI => request.header("x-goog-api-key", api_key),
        _ => request.bearer_auth(api_key),
    };
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(custom_header.trim())
        && let Some(map) = value.as_object()
    {
        for (key, val) in map {
            if let Some(header) = val.as_str() {
                request = request.header(key.as_str(), header);
            }
        }
    }

    let response = request.send().await.map_err(|e| format!("请求供应商 Models 接口失败：{e}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("读取供应商响应失败：{e}"))?;
    if !status.is_success() {
        return Err(format!(
            "供应商 Models 接口返回 {status}：{}",
            truncate(&body)
        ));
    }
    parse_model_ids(protocol_type, &body)
}

/// 解析远端响应为模型 ID 列表：OpenAI/Anthropic 取 `data[].id`，Gemini 取 `models[].name`。
pub fn parse_model_ids(protocol_type: i32, body: &str) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct OpenAiEnvelope {
        #[serde(default)]
        data: Vec<OpenAiModel>,
    }
    #[derive(Deserialize)]
    struct OpenAiModel {
        id: String,
    }
    #[derive(Deserialize)]
    struct GeminiEnvelope {
        #[serde(default)]
        models: Vec<GeminiModel>,
    }
    #[derive(Deserialize)]
    struct GeminiModel {
        name: String,
    }

    if protocol_type == PROTOCOL_GEMINI {
        let parsed: GeminiEnvelope = serde_json::from_str(body)
            .map_err(|e| format!("解析供应商响应失败：{e}"))?;
        Ok(parsed.models.into_iter().map(|m| m.name).collect())
    } else {
        let parsed: OpenAiEnvelope = serde_json::from_str(body)
            .map_err(|e| format!("解析供应商响应失败：{e}"))?;
        Ok(parsed.data.into_iter().map(|m| m.id).collect())
    }
}

fn truncate(body: &str) -> String {
    body.chars().take(ERROR_BODY_LIMIT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_models_url_appends_models_when_versioned() {
        assert_eq!(
            build_models_url("https://api.openai.com/v1", PROTOCOL_OPENAI_COMPATIBLE),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            build_models_url("https://api.minimax.io/anthropic/v1/", PROTOCOL_ANTHROPIC),
            "https://api.minimax.io/anthropic/v1/models"
        );
        assert_eq!(
            build_models_url("https://generativelanguage.googleapis.com/v1beta", PROTOCOL_GEMINI),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
    }

    #[test]
    fn test_build_models_url_appends_default_version_when_bare() {
        assert_eq!(
            build_models_url("https://api.openai.com", PROTOCOL_OPENAI_COMPATIBLE),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            build_models_url("https://generativelanguage.googleapis.com/", PROTOCOL_GEMINI),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
    }

    #[test]
    fn test_parse_openai_model_ids() {
        let ids =
            parse_model_ids(PROTOCOL_OPENAI_COMPATIBLE, r#"{"data":[{"id":"gpt-4o"},{"id":"o3"}]}"#)
                .unwrap();
        assert_eq!(ids, vec!["gpt-4o".to_string(), "o3".to_string()]);
    }

    #[test]
    fn test_parse_anthropic_model_ids() {
        let ids = parse_model_ids(
            PROTOCOL_ANTHROPIC,
            r#"{"data":[{"id":"claude-sonnet-4-5","type":"model"}]}"#,
        )
        .unwrap();
        assert_eq!(ids, vec!["claude-sonnet-4-5".to_string()]);
    }

    #[test]
    fn test_parse_gemini_model_ids() {
        let ids = parse_model_ids(
            PROTOCOL_GEMINI,
            r#"{"models":[{"name":"models/gemini-2.5-flash"}]}"#,
        )
        .unwrap();
        assert_eq!(ids, vec!["models/gemini-2.5-flash".to_string()]);
    }

    #[test]
    fn test_parse_invalid_body_is_error() {
        assert!(parse_model_ids(PROTOCOL_OPENAI_COMPATIBLE, "not json").is_err());
    }
}
