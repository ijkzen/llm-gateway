# 需求：导出备份 & 恢复备份（backup-restore）

## 来源请求（用户原话）

为项目添加导出备份和恢复备份的功能。功能入口在设置页面的修改密码按钮的左边，点击后弹入弹窗。弹窗上有两个按钮，一个是恢复备份，一个是导出备份。点击导出备份，以 JSON 格式导出当前所有的供应商，该供应商下面的所有模型，以及所有虚拟模型，以及虚拟模型下面的所有成员模型。恢复备份点击以后出现新的弹窗，之前的弹窗消失。新弹窗下面分两个区域：上半部分可拖拽把文件拖进去，下面是手动选择文件，最后是确认导入按钮。导入成功弹 Toast「成功导入」；导入失败或格式错误弹具体格式错误的错误弹窗。

## 范围（Scope）

1. **入口**：设置页 `PageHeader` 内、修改密码按钮左侧新增一个按钮（图标按钮），点击弹出备份弹窗。
2. **备份弹窗**：含两个按钮——「恢复备份」「导出备份」。
3. **导出**：后端接口产出 JSON 文件下载。内容 = 全部供应商（含其字段与明文密钥）+ 各供应商名下全部模型 + 全部虚拟模型 + 各虚拟模型名下全部成员模型 + API Key（明文）+ 系统设置。
4. **恢复（导入）**：点击「恢复备份」→ 当前弹窗消失，打开导入弹窗。导入弹窗 = 上部拖拽区 + 下部手动选择文件（`<input type="file">`）+ 底部「确认导入」按钮。
5. **结果反馈**：导入成功 → Toast「成功导入」；导入失败 / 格式错误 → 弹具体错误弹窗（说明错在哪、为什么）。

## 已拍板的决策（grilling 结论）

- **密钥导出为明文**：导出 JSON 内含供应商 `api_key`/`extra` 明文与 API Key 明文（后端解密后导出）。库中这些字段为服务端 AES 加密存储、换机器/换密钥不可解，纯密文导出无法恢复。文件需用户自行妥善保管。
- **恢复 = 整体替换**：清空现有全部供应商/模型/虚拟模型/API Key 后按备份重建（系统设置例外：只覆盖备份里出现的键，不删除当前键，避免清掉新版本种子键）。导入**事务性**执行（任一步失败整体回滚），导入前前端弹确认（文案明确列出将替换的配置类型 + 明文密钥警告）。会删除备份时间点之后新增的配置。
- **确认文案扩展**：破坏性确认明确写「将清空并替换全部供应商/虚拟模型/API Key/系统设置」并提示备份含明文密钥。

## 非目标（Non-goals，ponytail 裁剪）

- **不导出** cron 任务、会话、请求历史、登录用户（密码/会话是运行时身份）。
- 不给备份文件加密码/加密（明文密钥由用户保管文件）。
- 不做增量/差异恢复、不做导入前 dry-run 预览。
- 系统设置导入不删除当前键（只覆盖备份里出现的键）。
- 不新增数据库表、不做 schema 迁移（导入复用现有表）。
- 前端不引入拖拽库：用原生 HTML5 drag 事件 + 现有 shadcn Dialog + `<input type="file">`。

## 事实约束（实现前已确认）

- 供应商列表/详情 API 的 `api_key` 恒脱敏，明文仅通过 `GET /api/providers/{id}/api-key` 解密端点提供 → 导出/导入必须是**后端端点**直接读库，不能前端拼。
- 实体字段（见 `src/entity/`）：`provider`（name 唯一 / base_url / api_key / custom_header / protocol_type / billing_mode / extra / sort_order / proxy_enabled / proxy_addr / disabled_reason / enable）、`provider_model`（provider_id / provider_model_id / context_length / max_output_tokens / reasoning / tool_use / image_understand / video_understand / protocol_type 可空 / proxy_enabled / proxy_addr）、`virtual_model`（display_id 唯一 / enable / load_balancing_strategy / fallback_strategy / interface_type）、`virtual_model_item`（virtual_model_id / model_id / enable / cascade_disabled）、`api_key`（name 唯一 / key 加密 / key_hash / enable）、`setting`（key Text 主键 / value / type 0..=4）。
- 前端 i18n 用 react-i18next（`web/src/i18n`），UI 文案需走翻译 key。
