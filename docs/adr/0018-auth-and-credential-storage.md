# 0018 — 认证与凭证存储（管理端会话 + /v1 API Key）

## Status

accepted

## Context

服务有两个鉴权面：管理后台 `/api/*`（Cookie Session）与对外 `/v1/*`（Bearer API Key）。该设计从早期搭建起即定型，没有评审拍板记录，属「按现状固化」的契约文档——安全面一旦被后续改动悄悄放宽（明文落库、可逆 token、无过期等）将不可接受，本文把不可违背的存储与校验语义固定下来。

## Decision

1. **密码**：argon2id 哈希（`Argon2::default()`），永不明文落库、不可逆；首启无用户时走 `/api/auth/init` 初始化唯一管理员，之后登录/登出/改密见 `/api/auth` 路由。改密吊销除当前会话外的全部既有会话。
2. **会话**：登录成功生成随机 token，经 `lg_session` Cookie（HttpOnly、SameSite=Lax）下发，有效期 7 天（`SESSION_TTL_SECS`）；服务端 `session` 表**只存 token 的 SHA-256 摘要**与过期时间——数据库泄露拿不到可复用 token，中间件按摘要 O(1) 查找。
3. **/v1 API Key**：服务端生成形如 `lg-` + 32 位随机 hex 的明文密钥，创建响应一次性返回；库中存 AES-256-GCM 密文（`enc:v1:`，密钥与 extra 同源，见 ADR-0002）与明文密钥的 SHA-256 摘要（`key_hash`，迁移 7 + 启动回填 `auth::backfill_api_key_hashes`）。Bearer 鉴权把请求携带的明文 key 现场 SHA-256 后按 `key_hash` 精确查找（仅启用行），**鉴权路径不解密**；密文只服务于详情返回/掩码展示。列表/创建响应走掩码（保留前 3 后 4），明文仅详情接口返回。
4. **拦截范围**：`/api/*` 除 auth 的 status/init/login 与 `/api/healthz` 外全部要求会话；`/v1/*` 全部要求 Bearer；SPA 静态资源不服务端拦截（前端路由守卫负责跳转）。CORS 为 permissive（已知风险，部署到不可信网络前须收敛）。

## Consequences

- 存储层全部为不可逆/可加密形态：密码 argon2id、会话与 key 只存 SHA-256 摘要、key 本体 AES 密文——备份与数据库泄露不直接给出凭据；鉴权零解密，纯摘要查找。
- AES 密钥环境变量变更后：API Key 的 /v1 鉴权不受影响（key_hash 仍在），但详情/掩码无法解密（掩码退化为空串），需重新创建以便展示；provider extra 则必须重新填写（ADR-0002）。改密即时踢掉旧会话。
- 鉴权路径唯一：任何新端点要么走会话中间件要么走 Bearer 中间件，不允许裸奔。
