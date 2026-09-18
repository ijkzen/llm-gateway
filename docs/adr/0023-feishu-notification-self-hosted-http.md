# 0023 — 飞书通知客户端自研 HTTP，不引入 oapi-sdk-rust

## Status

accepted

## Context

供应商可用性的自动转换（额度门控停用/恢复、边界实测停用/恢复、连续失败熔断、自动恢复探测）需要对外发飞书消息。实现路径有两条：引入社区 Rust SDK `easy-4-rust/oapi-sdk-rust`（Cargo 包名 `lark-oapi`），或用项目已有的 `reqwest` 直调官方端点。SDK 是直觉上的「正规」选择，故需记录否决依据。

## Decision

自研 HTTP 客户端，不引入该 SDK。实测依据：

1. **该 SDK 默认 features 编译不过**：`src/event/mod.rs` 调用 `cipher` 0.4 的 API（`BlockDecryptMut`、`decrypt_padded_vec_mut`），而其 release 提交把 `cbc` 由 0.1 升到 0.2（连带 `cipher` 0.5），上游所有分支均未修复，仓库自身也编译失败。要用只能 `default-features = false, features = ["rustls-tls", "registration"]`。
2. **未发布**：不在 crates.io，无 tag 无 release，只能 git 依赖锁 commit；单作者、0 star、代码一次性投放，README 自述「非飞书官方 SDK」。
3. **需要的能力面很小**：三组官方端点共 6 个调用点——`POST /open-apis/auth/v3/tenant_access_token/internal`、`POST /open-apis/im/v1/messages?receive_id_type=…`、`POST accounts.feishu.cn/oauth/v1/app/registration`（`action=begin` / `action=poll`，OAuth 2.0 Device Authorization Grant）。`reqwest` 已是直接依赖，直调无需新增任何依赖。

被否决的备选：引入 SDK 但关掉 `events` feature（用一个默认配置都编译不过的未发布 pre-alpha 包换 6 个调用点的薄封装，不划算）；改用官方其他语言的 SDK（Rust 无官方版）。

## Consequences

- 扫码注册的轮询状态机由我们自己维护，三个易错点必须守住：HTTP 400 的响应体也要解析（`authorization_pending`/`slow_down`/`access_denied`/`expired_token` 均以 400 返回）、`slow_down` 时轮询间隔 +5s、`expires_in` 与 `expire_in` 两种字段名都接受。
- 不实现 `addons` 编码（gzip + base64url）：默认应用模板已含 `im:message:send_as_bot` 权限与机器人能力，无需额外声明。
- 只支持飞书国内版（`open.feishu.cn` / `accounts.feishu.cn`），不处理 `tenant_brand=lark` 的国际版域名切换。
- `tenant_access_token` 不做进程内缓存，每次发送现取（官方语义：剩余有效期 >30 分钟时返回同一 token）。
- 术语见 CONTEXT.md「通知渠道」「自动转换通知」「扫码创建」。
