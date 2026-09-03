# REQUIREMENTS — 供应商 extra 字段加密保存

原始请求（用户原话）：供应商 extra 字段加密保存，加解密方式参考项目现有方案，做好历史数据迁移。

## Scope

provider 表的 `extra` 列（JSON 字符串，含 ak/sk、refresh_token、oauth_token、cookie_cloud 系密码等敏感凭据，以及 usage/usage_type 等开关与自定义键）从明文存储改为**整段加密存储**，复用项目现有 `src/crypto` 的 AES-256-GCM 方案（`enc:v1:` 前缀），并在启动时一次性迁移历史明文数据。

## Decisions（经 grill 确认）

1. **加密粒度**：整段 extra JSON 串加密（不是逐键加密）。复用 `crypto::encrypt`/`crypto::decrypt`，零新增加密逻辑。
2. **API 暴露**：后端读库时解密，API（列表/详情）透明返回解密后的明文 extra —— 前端零改动，编辑弹窗照常展示/整体回传。加密只防数据库落盘泄露。
3. **迁移策略**：启动时一次性迁移（请求进来之前）。扫描全部 provider 行，把无 `enc:v1:` 前缀的明文 extra 加密写回。幂等可重试。
4. **无密钥降级**：未配置 `API_KEY_ENCRYPTION_KEY` 时明文存储 + warn，不迁移；配置密钥后下次启动自动完成迁移。
5. **迁移失败处理**：单行失败（历史数据损坏等）跳过 + warn，不阻塞启动；其余行正常迁移。
6. **写回路径容错**：`write_back_refresh_token`（轮换写回）、`backfill_provider_extra`（模板补齐）在解密失败时返回 Err，调用方记日志中止该行操作，不影响其他行。
7. **密钥轮换**：不支持。密钥变更后旧密文解密失败，报错提示重新填写（与现有 api_key 行为一致）。

## 受影响代码点（已调研）

- 写库：`src/routes/providers.rs`（创建 `:366`、更新 `:463`，extra 整体替换写入）；`src/usage/mod.rs::write_back_refresh_token`（`src/usage/mod.rs:131-160`，refresh_token 轮换写回）；`src/provider_template/mod.rs::backfill_provider_extra`（`:62-101`，模板补齐）。
- 读库：`src/usage/mod.rs`（`usage_enabled` 判读 `:60-65`、`query_provider_usage` 构造 Credentials `:80-96`）；`src/usage/persist.rs::refresh_all_usage`（`:112` 按 usage_enabled 筛选）；`src/proxy/failure_recheck.rs:69`；`src/routes/providers.rs:710/807`（用量与预估接口）。
- 响应/日志：`src/routes/providers.rs:80`（`from_model` 原样带出 extra）；`src/provider_repo.rs:38/61`（结构化日志原样输出 extra）。

## Non-goals（ponytail 裁剪）

- 不新增任何加密原语、不封装新的加解密类型 —— 直接复用 `crypto::encrypt`/`crypto::decrypt`。
- 不改前端（API 透明，前端零改动）。
- 不做密钥轮换/多密钥版本。
- 不做惰性迁移（读时升级），避免并发写竞争。
- 不改 `provider_template` 表的 extra（模板数据不含用户凭据，保持现状）。
- 不改 api_key 列的现有行为。

## Open questions

无（grilling 已全部钉死）。

## 领域文档动作

改动满足 ADR 三条标准（难逆转：迁移需密钥回退；无上下文惊讶；真实取舍：粒度/迁移/轮换均有替代），在 spec 阶段记录一条 ADR（`docs/adr/0002-encrypt-provider-extra.md`）。CONTEXT.md 无术语冲突，无需改动。
