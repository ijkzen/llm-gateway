# 01: 供应商 extra 写入加密、读取解密全链路

**What to build:** 供应商的 extra 字段（JSON 串）从明文改为整段 AES-256-GCM 加密存储（复用现有 `src/crypto` 的 encrypt/decrypt，`enc:v1:` 前缀）。三处写入口在落库前统一加密：创建/更新供应商、refresh_token 轮换写回、模板补齐写回。所有读路径先解密再使用：列表/详情接口透明返回解密后的明文 extra（前端零改动）、usage 开关判读、用量查询构造凭据、定时刷新筛选、失败复查筛选。加密只防数据库落盘泄露，API 层对前端保持透明。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 创建/更新供应商带 extra，落库值为 `enc:v1:` 密文，`crypto::decrypt` 可解回原 JSON（CRUD 集成测试）
- [ ] 列表/详情接口返回解密后的明文 extra
- [ ] 未配置密钥时行为不变（明文降级 + warn），不影响现有测试
- [ ] refresh_token 轮换写回后落库仍为密文且新 token 可解出（测试）
- [ ] 模板补齐写回后落库仍为密文（测试）
- [ ] usage 开关判读/用量查询/定时刷新/失败复查读密文 extra 时行为正确（解密后判读）
