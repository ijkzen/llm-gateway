# 04 — 后端集成测试

Status: ready-for-agent

## 任务

- `tests/virtual_models_integration.rs`：CRUD 全流程、校验失败（空 displayId/非法策略/空 items/不存在 modelId）、display_id 唯一冲突（创建 + 更新）、diff 更新保留成员 enable、级联删除释放成员后可重映射、供应商删除级联清理虚拟模型条目、互斥映射（创建冲突/更新冲突/编辑保留自身成员）。
- `tests/virtual_models_openai_integration.rs`：/v1 列表形状（OpenAI 字段、禁用不出现、无内部字段）、详情、404 错误格式、空列表。
- `cargo test` 全量回归。

## Comments

2026-08-29 完成。新增 12 个集成测试全部通过；全量 `cargo test` 149 个测试（102 单元 + 47 集成）全绿。
