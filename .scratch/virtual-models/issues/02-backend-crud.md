# 02 — 后端管理 CRUD 路由 /api/virtual-models

Status: ready-for-agent

## 任务

- 新建 `src/routes/virtual_models.rs`：GET/POST `/`、GET/PUT/DELETE `/{id}`；camelCase DTO；中文错误消息；`response::bad_request/not_found/db_error` 复用。
- 列表/详情响应内联成员明细（join provider_model + provider：供应商名称/enable、远端模型 ID、上下文长度、四项能力）。
- 校验：displayId 非空、策略取值合法、items 非空且 modelId 存在；display_id 唯一冲突 → 「虚拟模型 ID 已存在」；成员占用 → 「模型 X 已被其他虚拟模型使用」（编辑场景排除自身）。
- 更新 diff 语义：传入 items 为最终成员集合——移除被去掉的、插入新增（默认启用）、保留未变成员的 enable；items 缺省不修改成员。
- 删除事务内级联删成员；`src/routes/mod.rs` 挂载 `/api/virtual-models`。
- 供应商删除级联清理引用其模型的虚拟模型条目（改 `src/routes/providers.rs::delete_provider`，防悬空）。

## Comments

2026-08-29 完成。CRUD + diff 更新 + 三层互斥校验（前置查询 + 唯一索引兜底 + 冲突消息区分 display_id/model_id）落地；供应商删除链路补齐虚拟模型条目级联清理。
