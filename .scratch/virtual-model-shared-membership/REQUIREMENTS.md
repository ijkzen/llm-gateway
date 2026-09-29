# REQUIREMENTS — 虚拟模型共享映射（解除供应商模型互斥归属）

来源（用户原话）：修改一个限制：现在供应商模型不能同时出现在两个虚拟模型下面。我需要修改这个限制，允许一个供应商模型同时出现在两个虚拟模型下面。

## 范围（grilling 共识，2026-09-29）

1. **彻底移除互斥**：一个供应商模型可不限数量地同时加入多个虚拟模型。
2. **同一虚拟模型内保持去重**：同一虚拟模型内同一供应商模型仍只允许出现一次（现状行为不变）。
3. **备份导入同步放开**：`backup.rs` 的跨虚拟模型重复校验同步移除，导入导出与实时 API 规则一致。

## 需要拆除的限制（4 层，均为删除）

| 层 | 位置 | 处理 |
|---|---|---|
| DB 唯一索引 | `src/db.rs:259` `uq_virtual_model_items_model_id` | 迁移 28 drop 唯一索引，换普通索引（`model_id` 查询路径保留索引） |
| API 校验 | `src/routes/virtual_models.rs:289` `validate_item_model_ids` + :916 SQL 错误兜底 | 删除「已被其他虚拟模型使用」冲突检查（模型存在性校验保留） |
| 备份导入校验 | `src/backup.rs:373/473` | 删除跨虚拟模型重复检查（同一虚拟模型内去重语义保留） |
| 前端候选过滤 | `VirtualModelEditDialog` `mappedModelIds`（`web/src/components/virtual-models/`，页面计算） | 删除过滤；i18n `unusedEmptyHint` 等文案去掉「被其他虚拟模型占用」表述 |

## 非目标（ponytail 结论：无需裁剪，范围已是纯删除）

- 不加「已在他处使用」徽标/提示——未请求，不建。
- 不加「最多两个」计数校验（备选项被否，见 ADR-0026）。
- 不动同虚拟模型内去重（`dedupe_items` 保留）。
- 不动接口类型/协议匹配校验（`validate_members_match_interface_type` 语义不变）。

## 已解决的开放问题（grilling）

- 共享上限：不限数量（非「恰好两个」）。
- 同一虚拟模型内重复：保持禁止。
- 备份导入：同步放开。
- 级联语义（随供应商禁用/恢复、cascade_disabled）不变，多归属行自然按行处理；「删除虚拟模型即释放成员」语义随之失效（只删本虚拟模型条目）。

## 文档对齐（已随本阶段更新）

- `CONTEXT.md`：「互斥映射」词条替换为「共享映射」。
- `docs/adr/0026-shared-virtual-model-membership.md`：新建，取代 ADR-0020 决策 2。
- `docs/adr/0020-virtual-model-domain-invariants.md`：决策 2 标记被取代，Consequences 同步。
- `docs/user-guide.md:208-212`：随实现更新（行为文案与代码同改）。
