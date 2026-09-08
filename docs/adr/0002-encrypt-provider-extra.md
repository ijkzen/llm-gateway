# 0002 — 供应商 extra 字段加密存储

## Status

accepted

## Context

provider 表 `extra` 列（JSON 字符串）存储供应商的敏感凭据（ak/sk、refresh_token、oauth_token、cookie_cloud 系 password）与用量开关（usage/usage_type）。该字段目前明文落库，与已加密的 `api_key` 列安全姿态不一致。如有数据库备份泄露或文件系统越权，extra 中的敏感凭据会直接暴露。

## Decision

extra 字段改为整段 AES-256-GCM 加密存储，复用 `src/crypto` 的 `encrypt`/`decrypt`（`enc:v1:` 前缀 + base64 编码的 nonce‖ciphertext），密钥派生自 `API_KEY_ENCRYPTION_KEY`（SHA-256）。API 层透明解密返回，前端零改动。启动时一次性迁移历史明文行（`provider_repo::backfill_extra_encryption`：无 `enc:v1:` 前缀的写回加密，幂等，单行失败跳过加 warn）。未配置密钥时明文降级 + warn（与 api_key 行为一致），不迁移；配置密钥后下次启动自动完成。不支持密钥轮换。

所有 extra 写入口一律在落库前经 `crypto::encrypt` 统一加密：CRUD 创建/更新（`routes/providers.rs`）、动态凭据写回（`usage::write_back_extra_key`，只改单键）、模板补齐与历史回填（`provider_template` 的 `backfill_*_provider_extra` 系，随启动 `upsert_templates` 幂等对齐存量行）、备份还原导入（`backup.rs`）。新增写入口必须复用同一加密函数。

## Consequences

- 数据库落盘泄露场景下，extra 中敏感凭据受 AES-256-GCM 保护；认证标签防篡改。
- 前端编辑交互不变（API 返回解密后明文），写入时仍是整体替换 —— 前端在编辑弹窗中修改 extra 后整体 PUT，后端收到的是明文 JSON，加密发生在写入库的瞬间。
- 动态凭据写回（`usage::write_back_extra_key`）在解密失败时返回 Err，调用方中止本轮写回，避免在无法解密的密文上解析后只留下该键而清空其余凭据；模板补齐/历史回填（`provider_template::backfill_*_provider_extra`）与启动加密迁移（`provider_repo::backfill_extra_encryption`）解密或更新失败仅 warn 并跳过该行，不阻塞其余行。
- 密钥变更需重新填写 extra（`api_key` 列已有同样约束）。