# 05: 备份集成 —— 导出/导入通知配置

**What to build:** 备份文件带上通知渠道配置（含明文 appSecret），恢复时一并还原；导入**早于本特性**的旧备份文件时**不清掉**现有通知配置。

**Blocked by:** 01（需要通知渠道表与实体）

**Status:** ready-for-agent

- [ ] 导出：`BackupFile` 新增可选字段 `notificationChannels`，**恒产出**（可能为空数组）；`appSecret` 为**明文**——与其他凭据口径一致（备份必须能脱离原库密钥恢复，ADR-0017）
- [ ] 导入：`Some(list)` → 先清空 `notification_channel` 再插入（密文**重新加密**落库）；**`None`（旧文件缺该字段）→ 完全不触碰现有配置**
- [ ] 这是与「整体替换」语义的**唯一例外**，理由是避免老备份静默清掉配置；导入的其余语义不变
- [ ] 校验：`channel` 非空、`receiverType` ∈ {`open_id`,`email`}、`receiver` 非空、`appId` 非空；错误消息中文
- [ ] 集成测试（并入 `tests/notification_integration.rs`）：导出含通知配置且 `appSecret` 为明文；导入缺该字段的旧文件 → 现有配置**不被清掉**；导入含该字段 → **整体替换**
- [ ] 文档对齐：`docs/adr/0017-backup-export-and-restore.md` 已在本特性 Stage 1 更新（导出清单加入通知渠道、明确可选字段的导入语义），实现完成后回读确认与代码现状一致
