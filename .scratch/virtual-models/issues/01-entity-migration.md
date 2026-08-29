# 01 — 实体与迁移：virtual_model / virtual_model_item

Status: ready-for-agent

## 任务

- 新建 `src/entity/virtual_model.rs`：主键 `virtual_model_id`、`display_id`（unique）、`enable`、`load_balancing_strategy`、`fallback_strategy`、时间戳；定义 `LoadBalancingStrategy`（0=订阅制优先、1=按量付费优先、2=轮转、3=随机）与 `FallbackStrategy`（0=直接失败、1=依次重试其他启用成员）枚举（`DeriveActiveEnum`，i32 存 INTEGER）。
- 新建 `src/entity/virtual_model_item.rs`：主键 `virtual_model_item_id`、逻辑外键 `virtual_model_id` / `model_id`（→ `provider_model.model_id`）、`enable`、时间戳。
- `src/entity/mod.rs` 注册两个模块。
- `src/db.rs::migrate()`：provider_model 之后建两张表；migration 6 建 `idx_virtual_model_items_virtual_model_id` 与 `uq_virtual_model_items_model_id`（model_id 全局唯一 = 互斥映射）。

## Comments

2026-08-29 完成。两张实体表 + migration 6 落地，`cargo check`/`clippy` 通过；互斥映射由 model_id 全局唯一索引在 DB 层兜底。
