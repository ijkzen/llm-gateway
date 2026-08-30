# Security Policy

## Reporting a Vulnerability

llm-gateway 是个人维护的开源项目。请**不要**在公开 issue 中报告安全漏洞。

请通过以下方式私下报告：

- GitHub Security Advisory（推荐）：仓库首页 → Security → Report a vulnerability
- 或邮件联系维护者

## Scope

以下内容属于安全关注范围：

- API Key 的存储与传输（AES-256-GCM 加密、`lg-` 密钥鉴权）
- `/v1` 接口的越权访问
- 管理后台认证（argon2id、Cookie Session）
- 上游请求的 SSRF 风险

## Response

我们会尽快确认并修复漏洞，修复后发布公开说明。在修复发布前，请勿公开漏洞细节。
