# 02: 后端备份导入（整体替换）

**What to build:** 提供导入端点 `POST /api/backup/import`：解析+校验备份 JSON，单事务内整体替换供应商/模型/虚拟模型/成员/API Key 并 upsert 系统设置；成功返回计数，失败返回 400 + 具体（带路径）中文错误消息且数据不变。

**Blocked by:** 01（导入往返测试依赖 01 定义的导出格式）

**Status:** ready-for-agent

- [ ] `parse_backup` 纯函数：解析 + 结构校验，返回带路径的精确错误（缺字段、version 不支持、类型错、成员引用不存在的模型等）
- [ ] `apply_import` 单事务内按依赖序删除（成员→虚拟模型→模型→供应商→API Key）再重建；任一步失败整体回滚
- [ ] 设置 upsert：备份里出现的键覆盖 value/type；备份没有的键保留当前值不删除
- [ ] 复用既有校验：供应商字段/extra/protocol_billing/proxy、模型字段、虚拟模型策略与接口类型、成员协议匹配
- [ ] 重新加密入库：供应商 api_key/extra、API Key（含重算 key_hash）
- [ ] 纯函数单测：畸形输入错误路径、事务回滚
- [ ] 集成测试：空库导入完整重建（含 API Key 可鉴权、设置恢复）；有旧数据时整体替换且成员指向新 id；设置保留/覆盖；错误→400 + 具体消息且库未变

**Blocked by:** 01-backup-export
