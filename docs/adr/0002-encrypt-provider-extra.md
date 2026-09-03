# 0002 — 供应商 extra 字段加密存储

## Status

accepted

## Context

provider 表 `extra` 列（JSON 字符串）存储供应商的敏感凭据（ak/sk、refresh_token、oauth_token、cookie_cloud 系 password）与用量开关（usage/usage_type）。该字段目前明文落库，与已加密的 `api_key` 列安全姿态不一致。如有数据库备份泄露或文件系统越权，extra 中的敏感凭据会直接暴露。

## Decision

extra 字段改为整段 AES-256-GCM 加密存储，复用 `src/crypto` 的 `encrypt`/`decrypt`（`enc:v1:` 前缀 + base64 编码的 nonce‖ciphertext），密钥派生自 `API_KEY_ENCRYPTION_KEY`（SHA-256）。API 层透明解密返回，前端零改动。启动时一次性迁移历史明文行（无 `enc:v1:` 前缀的写回加密，幂等，单行失败跳过加 warn）。未配置密钥时明文降级 + warn（与 api_key 行为一致），不迁移；配置密钥后下次启动自动完成。不支持密钥轮换。

## Consequences

- 数据库落盘泄露场景下，extra 中敏感凭据受 AES-256-GCM 保护；认证标签防篡改。
- 前端编辑交互不变（API 返回解密后明文），写入时仍是整体替换 —— 前端在编辑弹窗中修改 extra 后整体 PUT，后端收到的是明文 JSON，加密发生在写入库的瞬间。
- 写回路径（`write_back_refresh_token`、`backfill_provider_extra`）在解密失败时返回 Err，调用方中止该行操作，避免凭据损坏。
- 密钥变更需重新填写 extra（`api_key` 列已有同样约束）。
- 三个写入口（CRUD 创建更新、refresh_token 轮换、模板补齐）都需要在写入前统一加密。