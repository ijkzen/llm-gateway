# Spec: 供应商 extra 字段加密存储

Labels: `ready-for-agent`

## Problem Statement

provider 表的 `extra` 列（JSON 字符串）存储供应商的敏感凭据（ak/sk、refresh_token、oauth_token、cookie_cloud 系 password），目前明文落库。数据库备份泄露或文件系统越权会导致这些凭据直接暴露，与已加密的 `api_key` 列安全姿态不一致。

## Solution

将 provider 表 `extra` 字段改为整段 AES-256-GCM 加密存储，复用 `src/crypto` 现有的 `encrypt`/`decrypt`（`enc:v1:` 前缀），API 层透明解密返回（前端零改动），启动时一次性迁移历史明文行，未配置密钥时明文降级 + warn。

## User Stories

1. 作为管理员，我配置供应商的敏感凭据（ak/sk、refresh_token、oauth_token 等）后，这些凭据在数据库中以密文存储，以便数据库备份泄露时凭据不被直接暴露。
2. 作为管理员，我通过 API 列表/详情接口查看供应商 extra 时，看到的是解密后的明文 JSON，以便前端编辑/展示不受影响。
3. 作为管理员，我的开发环境未配置 `API_KEY_ENCRYPTION_KEY` 时，extra 明文存储并收到 warn 日志，以便开发体验不受影响。
4. 作为管理员，我配置密钥后重启服务，已有的明文 extra 被自动迁移为密文，以便无需手动操作。
5. 作为管理员，启动迁移时某行 extra 因历史数据损坏无法迁移，该行被跳过并记录 warn，其余行正常迁移，服务正常启动，以便不阻塞业务。
6. 作为管理员，我更换了 `API_KEY_ENCRYPTION_KEY` 后，旧密文解密失败时收到报错提示我重新填写 extra，以便凭据可恢复。
7. 作为管理员，用量刷新任务自动轮换 refresh_token 后，新 token 仍以密文写入 extra，以便轮换后安全姿态不变。
8. 作为管理员，模板补齐写回 extra 后，落库仍为密文，以便安全姿态一致。

## Implementation Decisions

### 加密层

- **不再新增加解密函数或类型**。直接复用 `src/crypto/mod.rs` 的 `encrypt(plaintext: &str) -> String` 和 `decrypt(ciphertext: &str) -> anyhow::Result<String>`。
- `encrypt` 在未配置密钥时返回明文 + warn，`decrypt` 遇到无 `enc:v1:` 前缀的值作为历史明文原样返回 —— 这两条行为全部复用，零改动。
- 需要新增一个辅助函数 `is_encrypted(extra: &str) -> bool` 供迁移判断（检查是否以 `enc:v1:` 开头）。放在 `src/crypto/mod.rs` 或直接内联在迁移函数中。

### 写入口统一加密

三处写库路径，每处在写入前对 extra 调用 `crypto::encrypt`：

1. **`src/routes/providers.rs`**：创建（`create_provider`）和更新（`update_provider`）中，赋值 `extra: Set(req.extra)` 之前先加密。注意更新时 req.extra 是 `Option<String>`，`unwrap_or` 走现有逻辑不变。
2. **`src/usage/mod.rs::write_back_refresh_token`**：轮换 refresh_token 后，构建 `ActiveModel` 时对 `extra` 调用 `crypto::encrypt`。
3. **`src/provider_template/mod.rs::backfill_provider_extra`**：模板补齐后，写回 `am.extra` 之前调用 `crypto::encrypt`。

### 读路径透明解密

- **`src/routes/providers.rs::ProviderResponse::from_model`**：`extra: model.extra` 改为 `extra: crypto::decrypt(&model.extra).unwrap_or(model.extra)`。解密失败时保持原值（可读作密文，但用户可重新编辑保存）。
- **`src/provider_repo.rs`** 的日志输出 `extra = %model.extra`：extra 在日志中保持加密态落日志（日志本身可能泄密），但现有日志已是密文态可接受。
- **`src/usage/mod.rs`** 中 `usage_enabled`、`query_provider_usage` 等读 extra 的函数：需要在读之后先解密再 parse JSON。具体：
  - `usage_enabled(&model.extra)` → `usage_enabled(&crypto::decrypt(&model.extra).unwrap_or_default())`
  - `query_provider_usage` 中 `serde_json::from_str::<Value>(&model.extra)` → 先解密再 parse。
  - `write_back_refresh_token` 中从 DB 读到 extra 后先解密再 parse/merge/insert，写回前加密。
- **`src/usage/persist.rs::refresh_all_usage`**：筛选 `usage_enabled(&p.extra)` 之前先解密。
- **`src/proxy/failure_recheck.rs`**：`usage_enabled(&p.extra)` 之前先解密。

### 启动迁移

- 在 `src/lib.rs::init()` 中，`backfill_api_key_hashes` 之后、provider 模板 upsert 之前，挂载新迁移函数 `backfill_extra_encryption(&db)`。
- 实现：只读 `is_deleted = false` 的 provider 行，对 `extra` 不以 `enc:v1:` 开头的行调用 `crypto::encrypt` 写回。单行失败时 `tracing::warn!` 跳过，不阻塞整个迁移。
- 当 `encryption_enabled()` 为 false 时跳过（`tracing::info!` 日志说明未配置密钥，跳过迁移）。

### 写回路径容错

- `write_back_refresh_token` 中解密失败时返回 `UsageError::Auth`（调用方已处理该错误类型）。
- `backfill_provider_extra` 中解密失败时 `tracing::warn!` 跳过该行，不 abort 整个补齐流程。

## Testing Decisions

### 测试原则

- 只测试外部行为：数据库落库值可解密为预期明文（不关心内部 nonce/IV 细节），API 返回明文 extra。
- 迁移测试：明文行→密文可解、已密文行跳过、未配置密钥时跳过。

### 测试缝隙

1. **CRUD 集成测试**（`tests/providers_integration.rs`）：创建/更新 provider 带 extra → 断言落库值为 `enc:v1:` 密文且 `crypto::decrypt` 可解开 → 列表/详情接口返回解密后明文。
2. **启动迁移测试**（新文件或挂 `tests/` 下）：直接调用 `backfill_extra_encryption` → 明文行变为密文、已密文行不变（幂等）、未配置密钥时跳过。
3. **refresh_token 写回测试**（`tests/provider_usage_integration.rs` 或 `tests/providers_integration.rs`）：构造加密 extra → 调用 `write_back_refresh_token` → 断言落库 extra 仍为密文且新 token 可解出。
4. **模板补齐写回测试**（`src/provider_template/tests.rs`）：构造加密 extra → 调用 `backfill_provider_extra` → 断言落库仍为密文。

## Out of Scope

- 前端改动（API 透明，保持现状）。
- 密钥轮换/多密钥版本（与现有 api_key 策略一致，不支持）。
- 惰性迁移（读时升级），避免并发写竞争。
- `provider_template` 表 extra 的加密（模板不含用户凭据，保持现状）。
- `api_key` 列现有行为的任何改动。

## Further Notes

- 改动跨三个写入口（routes CRUD、usage write_back_refresh_token、provider_template backfill），每个入口都要统一加密，不能遗漏。
- 每条读路径要记得先解密再 parse JSON，否则 `usage_enabled` 等判读密文 JSON 会失败。
- 所有写路径要么收到前端明文（CRUD），要么「解密 → 改 → 再加密」（轮换写回、模板补齐），因此不会出现对已加密值再次 `encrypt` 的嵌套加密。迁移函数只处理无 `enc:v1:` 前缀的行，幂等可重试。
- 考虑 `decrypt` 失败时（如密钥变更），`ProviderResponse::from_model` 中降级为返回原始密文值（前端看到的是 `enc:v1:...` 字符串，可接受作为临时状态）。`usage_enabled` 解密失败时返回 false（相当于用量未开启，安全优先）。