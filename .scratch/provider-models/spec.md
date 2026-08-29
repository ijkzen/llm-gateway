# 供应商模型（Provider Models）

Status: ready-for-agent

2026-08-29 grill 会话定稿。术语见根目录 `CONTEXT.md`（供应商 / 供应商模型 / 模型目录 / 刷新 / 候选模型 / 智能填充 / 尾段匹配）。

## 背景

为「供应商」登记其可用的模型清单及能力元数据，支撑后续按能力选模型的场景。数据来源两条路：按协议调供应商自己的 Models 接口拉取（刷新），再用内嵌的模型目录做智能填充；或完全手动录入。

## 数据层

新表 `provider_model`（migration 5 只建索引，建表走 entity 的 `if_not_exists`）：

| 列 | 类型 | 约束 |
| --- | --- | --- |
| `model_id` | i32 | PK 自增 |
| `provider_id` | i32 | 逻辑外键 → `provider.id`；**供应商删除时应用层级联硬删**（本仓库无 DB 级 FK） |
| `provider_model_id` | String | NOT NULL；UNIQUE(provider_id, provider_model_id) |
| `context_length` | i64 | NOT NULL |
| `max_output_tokens` | i64 | NOT NULL |
| `reasoning` / `tool_use` / `image_understand` / `video_understand` | bool | NOT NULL default false |
| `created_at` / `updated_at` | DateTimeUtc | NOT NULL |

索引：`idx_provider_models_provider_id ON provider_model (provider_id)`（migration 5）。

## 模型目录

- 数据：models.dev `models.json`，2026-08-29 抓取，363 条，minified 后 vendor 在 `src/provider_model/data/models.json`（293KB）。
- 内嵌：`include_str!` 编译期打包 + `OnceLock` 惰性解析一次。不做运行时更新；过期时手动重新下载 minify 替换该文件。
- 决策记录：`docs/adr/0001-embed-model-catalog-as-vendored-asset.md`。
- 字段映射：`limit.context`→context_length、`limit.output`→max_output_tokens、`reasoning`→reasoning、`tool_call`→tool_use、`modalities.input` 含 `image`/`video`→image/video_understand。363 条中 8 条缺 `limit`（影响智能填充三态，见下）。

## 后端 API

依赖：新增 `reqwest`（default-features=false，features = json + rustls-tls）。

| 方法 路径 | 说明 |
| --- | --- |
| `GET /api/provider-models` | 全量列表（含 providerId），供页面按供应商分组渲染 |
| `GET /api/providers/{provider_id}/models` | 某供应商的模型列表 |
| `POST /api/providers/{provider_id}/models` | 手动添加单个（两个数字必填校验，> 0） |
| `POST /api/providers/{provider_id}/models/batch` | 批量导入；先查已存在并跳过，批内去重，事务插入 |
| `PUT /api/providers/{provider_id}/models/{model_id}` | 更新（全字段，含 provider_model_id，唯一冲突报 400） |
| `DELETE /api/providers/{provider_id}/models/{model_id}` | 删除单个 |
| `POST /api/providers/{provider_id}/models/refresh` | 刷新：调远端 Models 接口 + 服务端尾段匹配，返回候选 |

### refresh 细节

- URL 拼接：base_url 去尾部 `/`；若末段非版本段（`v1`/`v1beta`/`v1alpha`），按协议补默认版本段（OpenAI 兼容/OpenAI Response/Anthropic → `/v1`，Gemini → `/v1beta`），再拼 `/models`。种子模板的 base_url 普遍已带 `/v1`。
- 认证：OpenAI 系 `Authorization: Bearer`；Anthropic `x-api-key` + `anthropic-version: 2023-06-01`；Gemini `x-goog-api-key`。均附带供应商 `custom_header`（JSON 对象展开为请求头）。api_key 解密后全程不出后端。
- 响应解析：OpenAI/Anthropic 取 `data[].id`；Gemini 取 `models[].name`。
- 匹配（服务端做）：两边模型 ID 各按 `/` 分割取最后一段，忽略大小写精确相等。
- 候选三态：目录命中且 limit 完整 → `smart`（绿「已智能填充」）；命中但缺 `limit.context` 或 `limit.output` → `partial`（黄「信息不完整」，返回已有部分，缺失数字由前端补填）；未命中 → `manual`（「需手动填写」）。
- 已导入的模型（按 provider_model_id 忽略大小写）不出现在候选中；远端列表不做类型过滤。
- 超时 15s；远端非 2xx 时把状态码与截断后的错误消息透传为 400/502 中文错误。

## 前端

- 导航（`web/src/lib/pages.ts`）：`模型提供商` → **供应商**；新增 **供应商模型**（`/provider-models`）。
- 页面 `/provider-models`：每个供应商一个区块——顶行左侧供应商名称、右侧「添加」按钮，下方分割线，分割线下方模型卡片平铺；无供应商时空态引导。
- 卡片：模型 ID + 能力图标（lucide：reasoning/tool_use/image_understand/video_understand，仅展示为 true 的能力，带 tooltip）；点击卡片开详情弹窗。
- 详情弹窗：默认只读，右上「编辑」；点击后进入编辑态，「编辑」消失，右上变「删除」（走 ConfirmDialog）+「更新」；「更新」保存后回到只读；直接关闭弹窗丢弃未保存修改。
- 添加大弹窗（按供应商）：
  - 顶部「尝试刷新」→ 调 refresh 渲染候选卡片；卡片带勾选框，**全部不预选**。
  - 三态文案：绿「已智能填充」；黄「信息不完整」（内联补齐缺失数字后解锁勾选）；「需手动填写」（补齐两个数字后解锁勾选，能力开关默认 false）。
  - 底部「添加」批量导入选中项（无可选项时禁用）。
  - 弹窗内常驻「手动添加」表单区（模型 ID + 两个数字 + 四个能力开关），与刷新并存；刷新失败 toast 报错并引导用手动表单。
- 风格对齐 nyro 浅色拟态玻璃（亮/暗一致）。

## 测试

- 后端：目录解析与尾段匹配单测（含大小写、`models/` 前缀、缺 limit → partial）；CRUD/批量/去重集成测试；供应商删除级联删模型；refresh 的 URL 拼接与三态判定单测（HTTP 拉取与匹配拆开，拉取不做真实网络测试）。
- 前端：vitest 覆盖卡片能力图标、详情弹窗编辑态切换、添加弹窗三态与解锁逻辑、手动添加表单校验。

## grill 决策摘要（8 项）

1. 页面组织：每供应商一个区块，纵向堆叠。
2. 添加弹窗：刷新候选与手动添加表单并存。
3. 两个数字必填（NOT NULL）；目录命中但缺 limit → 黄色「信息不完整」，补齐后解锁。
4. 供应商删除 → 级联硬删其模型。
5. 刷新走后端代理 + 服务端匹配（新增 reqwest）。
6. 表名 provider_model，UNIQUE(provider_id, provider_model_id)。
7. 匹配规则：两边按 `/` 取最后一段忽略大小写比较；已导入的模型在刷新结果中不展示。
8. 候选卡片全部不预选。
