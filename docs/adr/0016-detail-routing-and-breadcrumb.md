# 0016 — 数据面板详情路由契约与结构面包屑

## Status

accepted

## Context

详情页 URL 曾用「字符串远端标识」拼装（如 `/models/:providerId/:modelId/overview`，模型 ID 需 encodeURIComponent 转义，全链路 7 处跳转）。而 `request` 表对 API Key 只存 name、不存 id，无法反查——若用 name 做路由，删除重建同名会混杂新旧历史，删除后历史也失去入口。面包屑各页自行拼装，同一页面从不同入口进入时 URL 会漂移。

## Decision

1. 详情 URL 一律用业务行**自增主键**：`/api-keys/:id/overview`、`/models/:modelId/overview`（provider_model 主键）；废弃字符串远端 ID 的旧 URL（直接断链，不兼容跳转）。删除后历史不可查是接受口径——request 表不为此引入 id 反查，避免同名重建混杂历史。
2. 行级跳转数据由服务端补齐：新增全局 `GET /api/provider-models/{model_id}` 主键单查（响应含 `providerName`）；rank/排行行补主键字段（模型行已删的历史聚合行为 null，前端据此禁用跳转）。
3. 面包屑 = 结构归属链（首页 › 所属列表[ › 所属供应商]），在 layout 顶栏集中式渲染：与入口无关、同 URL 恒定、不含当前页。
4. 详情页以主键 detail 驱动后续统计请求，detail 失败（已删/非法 id）走 404 错误态。

## Consequences

- URL 稳定可深链、可分享，转义/还原逻辑消灭；删除语义明确（历史不可查、行禁点）。
- 面包屑单处实现，新增详情页只声明归属链；rank 行对已删实体天然禁点。
- 详情页数据依赖收敛为「主键 → detail → 统计」单向链，页面不再持有完整对象入口。

相关：`.scratch/api-key-overview-nav/`、`.scratch/provider-detail-overview-nav/` 及模型面板主键路由改动。
