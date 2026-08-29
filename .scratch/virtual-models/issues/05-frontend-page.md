# 05 — 前端基座与页面

Status: ready-for-agent

## 任务

- `web/src/lib/pages.ts`：新增 `VIRTUAL_MODELS_PAGE`（/virtual-models、「虚拟模型」、Layers 图标），插在供应商模型之后。
- `web/src/App.tsx`：lazy 路由 `/virtual-models`。
- `web/src/lib/constants.ts`：`LOAD_BALANCING_STRATEGIES` / `FALLBACK_STRATEGIES` 文案映射与 label 函数。
- `web/src/hooks/use-virtual-models.ts`：类型（VirtualModel/VirtualModelItem/Payload）+ `virtualModelKeys` + CRUD hooks（invalidate all）。
- `web/src/pages/virtual-models.tsx`：PageHeader（刷新 + 添加虚拟模型）→ Separator → 平铺卡片 grid；loading/error/空态三态；详情目标按 id 派生（invalidate 后拿到最新数据）。
- `web/src/components/virtual-models/VirtualModelCard.tsx`：display_id + 启停状态点 + 策略 badge + 成员数/可用数。

## Comments

2026-08-29 完成。更新类型按 providers 页惯例拆分 `VirtualModelPayload` / `UpdateVirtualModelPayload`（字段均可选，匹配后端 diff 语义）。
