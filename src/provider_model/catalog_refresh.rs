//! 模型目录刷新：抓取 models.dev 目录、校验、原子写入外置文件（ADR-0025）。
//!
//! 源 URL 与目标路径均为参数（生产 handler 传 [`MODELS_URL`] 与配置的外置路径，
//! 测试传本地 mock URL 与临时文件），不新增环境变量。

use std::path::Path;
use std::time::Duration;

use crate::provider_model::catalog;

/// models.dev 全量目录源（与本仓库 vendor 文件同格式、同 serde 结构）。
pub const MODELS_URL: &str = "https://models.dev/models.json";

const TIMEOUT_SECS: u64 = 30;

/// 抓取 `url` 的目录内容，校验为合法且非空 JSON 后原子替换 `path`，返回条目数。
///
/// 任何失败（网络错误 / 非 2xx / JSON 非法 / 空目录）都返回错误且**不触碰** `path`。
pub async fn refresh_from(url: &str, path: &Path) -> Result<usize, String> {
    // 独立客户端（跟随重定向、每天一次）而非复用 usage::http（禁重定向、按代理缓存）：
    // 目录源是公开静态文件，语义与用量抓取不同。
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .user_agent(concat!("llm-gateway/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求模型目录失败：{e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("模型目录响应状态异常：{status}"));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("读取模型目录响应失败：{e}"))?;
    // 校验复用读取侧同一 serde 结构，结构不兼容/空目录即失败。
    let count = catalog::validate(&body)?;
    write_atomic(path, &body).await?;
    Ok(count)
}

/// 临时文件 + `rename` 原子替换（必要时先创建父目录）。
async fn write_atomic(path: &Path, body: &str) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建模型目录所在目录失败：{e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, body)
        .await
        .map_err(|e| format!("写入模型目录临时文件失败：{e}"))?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("替换模型目录失败：{e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const VALID: &str = "{\"acme/foo\":{\"name\":\"Foo\",\"reasoning\":false,\
                         \"tool_call\":true,\"limit\":{\"context\":1,\"output\":2}}}";

    /// 单次响应的 mock HTTP：返回 `status_line` + `body`，然后关闭连接。
    async fn serve_once(status_line: &'static str, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        });
        format!("http://{addr}/models.json")
    }

    /// 失败分支：预置旧文件，断言失败后内容不变。
    async fn assert_failure_preserves_old(url: &str) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(&path, "OLD").unwrap();
        let err = refresh_from(url, &path).await.expect_err("应失败");
        assert!(!err.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "OLD");
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[tokio::test]
    async fn refresh_writes_valid_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let url = serve_once("200 OK", VALID).await;

        let count = refresh_from(&url, &path).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[tokio::test]
    async fn refresh_rejects_invalid_json() {
        let url = serve_once("200 OK", "{ not json").await;
        assert_failure_preserves_old(&url).await;
    }

    #[tokio::test]
    async fn refresh_rejects_empty_object() {
        let url = serve_once("200 OK", "{}").await;
        assert_failure_preserves_old(&url).await;
    }

    #[tokio::test]
    async fn refresh_rejects_non_2xx() {
        let url = serve_once("500 Internal Server Error", "boom").await;
        assert_failure_preserves_old(&url).await;
    }

    #[tokio::test]
    async fn refresh_reports_network_error() {
        // 未监听的本地端口 → 连接失败。
        assert_failure_preserves_old("http://127.0.0.1:1/models.json").await;
    }
}
