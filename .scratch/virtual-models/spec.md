# 虚拟模型（Virtual Models）

Status: ready-for-agent

2026-08-29 需求会话定稿。术语见根目录 `CONTEXT.md`（虚拟模型 / display_id / 虚拟模型条目 / 互斥映射 / 负载均衡策略 / 降级策略）。

## 背景

不同供应商可能提供同一个模型的访问。作为 AI Gateway，需要把多个供应商模型映射为一个「虚拟模型」统一暴露给客户端，由网关在背后做负载均衡与降级。本次范围：数据模型、管理 CRUD、OpenAI 兼容的模型列表/详情接口与前端管理页面；**不实现实际转发（/v1/chat/completions）与真实的均衡/降级执行**，策略仅存储与展示。

## 数据层

新表 `virtual_model`（建表走 entity 的 `if_not_exists`）：

| 列 | 类型 | 约束 |
| --- | --- | --- |
| `virtual_model_id` | i32 | PK 自增 |
| `display_id` | String | NOT NULL；UNIQUE（客户端调用用的模型名） |
| `enable` | bool | NOT NULL default true；禁用后不出现在 /v1/models |
| `load_balancing_strategy` | i32 | NOT NULL default 0：0=订阅制优先、1=按量付费优先、2=轮转、3=随机 |
| `fallback_strategy` | i32 | NOT NULL default 0：0=直接失败、1=依次重试其他启用成员 |
| `created_at` / `updated_at` | DateTimeUtc | NOT NULL |

新表 `virtual_model_item`：

| 列 | 类型 | 约束 |
| --- | --- | --- |
| `virtual_model_item_id` | i32 | PK 自增 |
| `virtual_model_id` | i32 | 逻辑外键 → `virtual_model`；虚拟模型删除时应用层级联硬删 |
| `model_id` | i32 | 逻辑外键 → `provider_model.model_id`；**UNIQUE（互斥映射：一个供应商模型最多归属一个虚拟模型）** |
| `enable` | bool | NOT NULL default true；实际可用性 = 条目 enable ∧ 所属供应商 enable |
| `created_at` / `updated_at` | DateTimeUtc | NOT NULL |

索引（migration 6）：`idx_virtual_model_items_virtual_model_id`（按虚拟模型查成员）、`uq_virtual_model_items_model_id`（model_id 全局唯一）。

## 后端 API

管理端（统一 `{code,msg,data}` 响应，camelCase DTO，中文错误消息）：

| 方法 路径 | 说明 |
| --- | --- |
| `GET /api/virtual-models` | 全量列表（含成员明细：供应商名称/启用状态、远端模型 ID、能力等），管理视图含禁用项 |
| `POST /api/virtual-models` | 创建：displayId + 策略 + items（`{modelId, enable?}`，缺省启用）；displayId/策略/modelId 存在性校验 |
| `GET /api/virtual-models/{id}` | 详情 |
| `PUT /api/virtual-models/{id}` | 更新：items 传入时以该集合为最终成员做 diff（移除被去掉的、插入新增的、保留未变成员的 enable）；缺省表示不修改成员 |
| `DELETE /api/virtual-models/{id}` | 删除：级联删成员（成员模型被释放后可再映射） |

校验与约束：

- display_id 唯一冲突 → 400「虚拟模型 ID 已存在」；成员 model_id 唯一索引冲突 → 400「部分模型已被其他虚拟模型使用」。
- 互斥映射：创建/更新前先查占用（排除编辑目标自身），占用即 400「模型 X 已被其他虚拟模型使用」。
- 供应商删除时级联清理引用其模型的虚拟模型条目（防悬空），该虚拟模型的成员列表随之变空。

OpenAI 兼容（`/v1`，serde_json 直出，不走内部响应包装，暂不鉴权）：

| 方法 路径 | 说明 |
| --- | --- |
| `GET /v1/models` | `{object:"list", data:[{id, object:"model", created, owned_by:"llm-gateway"}]}`，只含启用的虚拟模型 |
| `GET /v1/models/{display_id}` | 单个模型对象；不存在或已禁用 → 404 `{"error":{message,type:"invalid_request_error",code:"model_not_found"}}` |

## 前端

> 2026-08-29 修订：按用户反馈重构为「区块式」交互（对齐供应商模型页骨架），原「小卡片网格 + 详情弹窗」方案废弃。

- 导航（`web/src/lib/pages.ts`）：新增 **虚拟模型**（`/virtual-models`，Layers 图标），位于供应商模型之后。
- 页面 `/virtual-models`：整体骨架与供应商模型页一致——顶部 PageHeader（左标题、右「刷新」+「添加虚拟模型」），下方**每个虚拟模型一个区块**；无虚拟模型时显示空态引导（即使已导入供应商模型）。
- 区块（对应供应商模型页的供应商区块）：
  - 顶行左上：虚拟模型名称（display_id）+「已禁用」badge（禁用时）+ 负载均衡/降级策略 badge；右上「⋯」菜单，仅两项：「编辑」「删除」（删除走二次确认弹窗）。
  - 分割线下方**平铺成员卡片**（纯展示、不可点击）：模型 ID + 能力图标 + 供应商名；已停用成员带「已停用」标记、供应商禁用带「随供应商禁用」标记（整卡置灰）。
- 创建/编辑弹窗（共用，**暂存模式**——弹窗内所有修改先改本地状态，点「保存/创建」一次性提交，取消即丢弃）：
  - 顶部基本信息：模型 ID、负载均衡策略（默认订阅制优先）、降级策略（默认直接失败）、虚拟模型启用开关（禁用即从 /v1/models 隐藏）。
  - 下方**按供应商分组**管理成员：每组顶行供应商名（禁用供应商带标记）+「添加」按钮；成员行 = 模型 ID + ctx + 能力图标 + 启停 Switch + 移除按钮。
  - 「添加」展开该供应商候选区（未被其他虚拟模型占用且未加入暂存的模型；点击即加入暂存并从候选消失）；候选为空时「添加」按钮禁用。
  - 底部「已选 N 个成员模型」（为 0 时提示「至少保留一个成员模型」并禁用提交）。
- 删除确认弹窗：ConfirmDialog 包装，说明成员将被释放。
- 风格对齐 nyro 浅色拟态玻璃（亮/暗一致），复用供应商模型页的卡片与分组交互模式。

## 测试

- 后端：CRUD/校验失败/display_id 唯一/diff 更新保留 enable/级联删除集成测试；互斥映射全链路（创建冲突、更新冲突、编辑保留自身成员、删除释放后可重映射）；/v1 列表形状、禁用模型不出现、404 错误格式。
- 前端：vitest 覆盖页面三态/空态/弹窗联动、编辑弹窗互斥排除与 enable 保留、详情弹窗启停切换与删除确认。

## 决策摘要（4 项）

1. `/v1/models/{display_id}` 详情返回**标准 OpenAI 格式**（仅 id/object/created/owned_by），不附内部扩展字段。
2. 虚拟模型本身有 `enable` 开关（默认启用），禁用的虚拟模型不出现在 /v1/models。
3. 负载均衡策略枚举顺序：0=订阅制优先、1=按量付费优先、2=轮转、3=随机。
4. 互斥映射：一个供应商模型最多归属一个虚拟模型，落到 DB 唯一索引 + 后端校验 + 前端列表排除三层。
