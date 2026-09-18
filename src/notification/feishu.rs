//! 飞书 Open API 客户端（手写 HTTP，ADR-0023）。
//!
//! 只做两件事：取 `tenant_access_token`、发文本消息。扫码注册走账号域，
//! 见 `register` 子模块。

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};

use super::FeishuConfig;

/// 生产基址（只支持飞书国内版，不做 Lark 国际版域名切换）。
const FEISHU_OPEN_BASE: &str = "https://open.feishu.cn";
const FEISHU_ACCOUNTS_BASE: &str = "https://accounts.feishu.cn";

/// 集成测试重定向：非空时替换飞书基址（mock 服务器按路径区分端点）。
pub const BASE_OVERRIDE_ENV: &str = "LLM_GATEWAY_FEISHU_BASE_URL";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

fn base_override() -> Option<String> {
    std::env::var(BASE_OVERRIDE_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Open API 基址。每次读环境变量（而非构造时快照），集成测试才能在用例内重定向。
pub fn open_base() -> String {
    base_override().unwrap_or_else(|| FEISHU_OPEN_BASE.to_string())
}

/// 账号域基址（扫码注册用，与 Open API 同一重定向变量）。
pub fn accounts_base() -> String {
    base_override().unwrap_or_else(|| FEISHU_ACCOUNTS_BASE.to_string())
}

/// 进程级复用的 HTTP 客户端（连接池与 TLS 会话随客户端存活）。
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// 飞书业务错误码 → 中文提示（带原始 code，便于对照开放平台文档）。
pub fn error_message(code: i64, msg: &str) -> String {
    let hint = match code {
        230027 => "飞书应用缺少发送消息权限（im:message:send_as_bot）",
        230006 => "飞书机器人能力未启用",
        230013 => "接收者不在机器人的可用范围内",
        230034 => "接收者 ID 非法",
        _ => return format!("飞书接口返回错误：code={code} msg={msg}"),
    };
    format!("{hint}（code={code}）")
}

/// 消息请求体。`content` 是 JSON **字符串**（不是对象）——飞书 im/v1/messages
/// 的契约，写成对象会被拒。
pub fn text_message_body(receiver: &str, text: &str) -> Value {
    json!({
        "receive_id": receiver,
        "msg_type": "text",
        "content": json!({ "text": text }).to_string(),
    })
}

/// 解析飞书响应：HTTP 非 2xx 也读 body（业务错误码通常随 200 返回，但网关层
/// 错误可能带 4xx/5xx，两种都要给出可读原因）。`code` 缺失按失败处理（fail-closed）。
async fn parse_reply(response: reqwest::Response, action: &str) -> Result<Value, String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("{action}失败：读取飞书响应出错（{e}）"))?;
    let body: Value = serde_json::from_str(&text)
        .map_err(|_| format!("{action}失败：飞书返回非 JSON 响应（HTTP {status}）"))?;

    match body.get("code").and_then(Value::as_i64) {
        Some(0) => {}
        Some(code) => {
            return Err(error_message(
                code,
                body.get("msg").and_then(Value::as_str).unwrap_or(""),
            ));
        }
        None => {
            return Err(format!(
                "{action}失败：飞书响应缺少 code 字段（HTTP {status}）"
            ));
        }
    }
    if !status.is_success() {
        return Err(format!("{action}失败：飞书返回 HTTP {status}"));
    }
    Ok(body)
}

/// 取 `tenant_access_token`。**每次现取不做缓存**：官方语义下剩余有效期 >30 分钟
/// 时返回同一 token，通知频率低，不值得维护进程内缓存。
pub async fn tenant_access_token(app_id: &str, app_secret: &str) -> Result<String, String> {
    let url = format!(
        "{}/open-apis/auth/v3/tenant_access_token/internal",
        open_base()
    );
    let response = client()
        .post(&url)
        .json(&json!({ "app_id": app_id, "app_secret": app_secret }))
        .send()
        .await
        .map_err(|e| format!("请求飞书 tenant_access_token 失败：{e}"))?;

    let body = parse_reply(response, "请求飞书 tenant_access_token").await?;
    body.get("tenant_access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "飞书未返回 tenant_access_token".to_string())
}

/// 发一条文本消息。
pub async fn send_text(
    token: &str,
    receiver_type: &str,
    receiver: &str,
    text: &str,
) -> Result<(), String> {
    let url = format!(
        "{}/open-apis/im/v1/messages?receive_id_type={receiver_type}",
        open_base()
    );
    let response = client()
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&text_message_body(receiver, text))
        .send()
        .await
        .map_err(|e| format!("发送飞书消息失败：{e}"))?;

    parse_reply(response, "发送飞书消息").await.map(|_| ())
}

/// 取 token 并发送一条文本消息（测试发送与自动通知共用入口）。
pub async fn send(config: &FeishuConfig, text: &str) -> Result<(), String> {
    let token = tenant_access_token(&config.app_id, &config.app_secret).await?;
    send_text(&token, &config.receiver_type, &config.receiver, text).await
}

/// 发一个 form-urlencoded POST，返回（HTTP 状态码，解析后的 JSON body）。
///
/// 注册流程专用：它的错误信号在 body 的 `error` 字段而非 `code`，且
/// `authorization_pending` 等**以 HTTP 400 返回**，所以不能走 `parse_reply`
/// 的「非 2xx 即失败」判定——状态码只作为诊断信息回给调用方。
pub(crate) async fn post_form(url: &str, fields: &[(&str, &str)]) -> Result<(u16, Value), String> {
    let response = client()
        .post(url)
        .form(fields)
        .send()
        .await
        .map_err(|e| format!("请求飞书失败：{e}"))?;

    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|e| format!("读取飞书响应出错：{e}"))?;
    let body: Value = serde_json::from_str(&text)
        .map_err(|_| format!("飞书返回非 JSON 响应（HTTP {status}）"))?;
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `content` 必须是 JSON 字符串——写成对象是这条链路最容易犯的错。
    #[test]
    fn message_body_encodes_content_as_json_string() {
        let body = text_message_body("ou_x", "你好\n第二行");

        assert_eq!(body["receive_id"], "ou_x");
        assert_eq!(body["msg_type"], "text");
        let content = body["content"].as_str().expect("content 必须是字符串");
        let parsed: Value = serde_json::from_str(content).unwrap();
        assert_eq!(parsed["text"], "你好\n第二行");
    }

    /// 已知错误码给中文提示，未知码兜底带原始 code 与 msg。
    #[test]
    fn error_codes_map_to_chinese_hints() {
        for (code, needle) in [
            (230027, "权限"),
            (230006, "机器人能力"),
            (230013, "可用范围"),
            (230034, "接收者"),
        ] {
            let message = error_message(code, "raw");
            assert!(
                message.contains(needle),
                "code={code} 应提示「{needle}」，实际：{message}"
            );
            assert!(message.contains(&code.to_string()), "应带上原始 code");
        }

        let unknown = error_message(99999, "boom");
        assert!(unknown.contains("99999") && unknown.contains("boom"));
    }

    /// 未设置重定向时用生产基址。
    #[test]
    fn open_base_defaults_to_feishu() {
        temp_env::with_vars([(BASE_OVERRIDE_ENV, None::<&str>)], || {
            assert_eq!(open_base(), FEISHU_OPEN_BASE);
        });
        temp_env::with_vars([(BASE_OVERRIDE_ENV, Some("http://127.0.0.1:9"))], || {
            assert_eq!(open_base(), "http://127.0.0.1:9");
        });
        temp_env::with_vars([(BASE_OVERRIDE_ENV, Some("   "))], || {
            assert_eq!(open_base(), FEISHU_OPEN_BASE, "空白值应视为未设置");
        });
    }
}
