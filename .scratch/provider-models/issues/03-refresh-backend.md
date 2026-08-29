# 03 — 刷新：远端 Models 接口调用

Status: ready-for-agent

- Cargo.toml 加 `reqwest`（default-features=false，features=json+rustls-tls）。
- `src/provider_model/refresh.rs`：
  - `build_models_url(base_url, protocol_type)`：去尾 `/`，末段非版本段（v1/v1beta/v1alpha）时按协议补 `/v1`（0/1/2）或 `/v1beta`（3），拼 `/models`。
  - 请求头：Bearer / x-api-key+anthropic-version / x-goog-api-key，展开 custom_header（JSON 对象），api_key 由调用方解密后传入。
  - 解析：OpenAI/Anthropic `data[].id`；Gemini `models[].name`。
  - 超时 15s；非 2xx 返回带状态码与截断错误消息的错误。
- 单测只测 URL 拼接与响应解析（不做真实网络请求）。

## Comments

- 2026-08-29 完成。后端实现 + 测试全绿（cargo test 102 单测 + 12 新集成测试，clippy 0 警告），并在 4027 端口真实服务冒烟验证（创建/批量/刷新错误透传/级联删除）。
