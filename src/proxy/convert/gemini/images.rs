use super::*;
use base64::Engine as _;

const MAX_INLINE_IMAGE_BYTES: usize = 20 * 1024 * 1024;

const INLINE_IMAGE_MIME_TYPES: &[&str] = &["image/jpeg", "image/png", "image/gif", "image/webp"];

fn is_remote_http_uri(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

/// Gemini 原生接受的 fileData URI（GCS 与 Files API），无需下载内联。
fn is_native_file_uri(uri: &str) -> bool {
    uri.starts_with("gs://")
        || uri.starts_with("https://generativelanguage.googleapis.com/")
        || uri.starts_with("http://generativelanguage.googleapis.com/")
}

fn build_image_client(proxy: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15));
    if let Some(proxy_addr) = proxy {
        let proxy = reqwest::Proxy::all(proxy_addr).map_err(|e| e.to_string())?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| e.to_string())
}

async fn fetch_image_inline(
    client: &reqwest::Client,
    url: &str,
) -> Result<(String, String), String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();
    if !INLINE_IMAGE_MIME_TYPES.contains(&mime.as_str()) {
        return Err(format!("不支持的图片类型：{mime}"));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(format!("图片超过 {} 字节上限", MAX_INLINE_IMAGE_BYTES));
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((mime, data))
}

/// 把 contents 中指向任意 http(s) URL 的 fileData 图片下载后转为 inlineData。
///
/// Gemini 的 fileData 仅接受 GCS / Files API URI，任意 http(s) 图片 URL 会被
/// 上游 400（LiteLLM 采取同样的下载内联策略）。下载失败移除该 part 并告警，
/// 不阻塞请求；先全部下载再按 part 下标倒序应用，避免移除时的索引位移。
pub async fn inline_remote_images(body: &mut Value, proxy: Option<&str>, request_id: &str) {
    let Some(contents) = body.get_mut("contents").and_then(Value::as_array_mut) else {
        return;
    };
    let mut targets: Vec<(usize, usize, String)> = Vec::new();
    for (content_index, content) in contents.iter().enumerate() {
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            continue;
        };
        for (part_index, part) in parts.iter().enumerate() {
            let Some(uri) = part.pointer("/fileData/fileUri").and_then(Value::as_str) else {
                continue;
            };
            if is_remote_http_uri(uri) && !is_native_file_uri(uri) {
                targets.push((content_index, part_index, uri.to_string()));
            }
        }
    }
    if targets.is_empty() {
        return;
    }

    let client = match build_image_client(proxy) {
        Ok(client) => client,
        Err(e) => {
            tracing::warn!(request_id, "图片下载客户端构建失败，移除全部远程图片：{e}");
            for (content_index, part_index, _) in targets.iter().rev() {
                remove_part(body, *content_index, *part_index);
            }
            return;
        }
    };
    let mut results = Vec::with_capacity(targets.len());
    for (content_index, part_index, url) in &targets {
        let inline = match fetch_image_inline(&client, url).await {
            Ok(inline) => Some(inline),
            Err(e) => {
                tracing::warn!(request_id, url, "下载远程图片失败，已移除该图片：{e}");
                None
            }
        };
        results.push((*content_index, *part_index, inline));
    }
    for (content_index, part_index, inline) in results.into_iter().rev() {
        let Some(parts) = body
            .get_mut("contents")
            .and_then(Value::as_array_mut)
            .and_then(|contents| contents.get_mut(content_index))
            .and_then(|content| content.get_mut("parts"))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        match inline {
            Some((mime, data)) => {
                parts[part_index] = json!({"inlineData": {"mimeType": mime, "data": data}});
            }
            None => {
                parts.remove(part_index);
            }
        }
    }
}

fn remove_part(body: &mut Value, content_index: usize, part_index: usize) {
    if let Some(parts) = body
        .get_mut("contents")
        .and_then(Value::as_array_mut)
        .and_then(|contents| contents.get_mut(content_index))
        .and_then(|content| content.get_mut("parts"))
        .and_then(Value::as_array_mut)
    {
        parts.remove(part_index);
    }
}
