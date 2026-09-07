# Spec: 导出备份 & 恢复备份（backup-restore）

Status: ready-for-agent

## Problem Statement

管理员在设置页只能逐项维护供应商/模型/虚拟模型，无法整体备份或迁移配置。换机器、误删、迁移部署时没有一次性导出/恢复整组配置的手段。需求：从设置页一键导出全部供应商（含明文密钥）及其模型、全部虚拟模型及其成员、API Key 与系统设置为 JSON；并可把该 JSON 整体恢复（替换）回系统。

## Solution

设置页 `PageHeader` 修改密码按钮左侧新增入口按钮。点击弹出备份弹窗，内含两个按钮「导出备份」「恢复备份」：

- **导出备份**：后端读库产出一个 JSON 文件（供应商含明文 `api_key` 与完整 `extra`；API Key 含明文密钥），前端触发下载。
- **恢复备份**：当前弹窗关闭，打开导入弹窗。导入弹窗上半部分可拖拽文件、下半部分手动选择文件，底部「确认导入」按钮。导入前弹确认（整体替换属破坏性操作，明确列出将替换的配置类型）；成功后 Toast「成功导入」；失败或格式错误弹具体错误弹窗。

## User Stories

1. 作为管理员，我想在设置页修改密码按钮左侧看到备份入口，以便不离开设置页即可备份/恢复配置。
2. 作为管理员，我想点击「导出备份」下载一个 JSON 文件，以便完整保存当前所有供应商、各供应商名下全部模型、所有虚拟模型及其成员、API Key 与系统设置。
3. 作为管理员，我希望导出文件里的 API Key 与 extra 是明文可用的，以便换机器恢复后无需重新逐项填写密钥。
4. 作为管理员，我想点击「恢复备份」进入导入弹窗，以便把之前导出的文件导回系统。
5. 作为管理员，我想把文件拖进导入弹窗，以便快速选择备份文件。
6. 作为管理员，我想通过文件选择器手动选文件，以便在拖拽不可用时仍能导入。
7. 作为管理员，我点击「确认导入」前系统会明确提示将清空并替换现有全部供应商/虚拟模型/API Key/系统设置，以便避免误操作。
8. 作为管理员，导入成功后我看到「成功导入」Toast 且各配置列表刷新，以便确认恢复完成。
9. 作为管理员，导入失败或文件格式错误时我看到具体的错误弹窗（说明哪里错、为什么），以便修正备份文件。
10. 作为管理员，我希望导入是事务性的——任一条数据非法时整体回滚、不产生半导入状态，以便失败后系统保持原样。
11. 作为管理员，恢复后 API Key 的明文会重新加密入库并重新计算哈希，以便 /v1 Bearer 鉴权继续可用。

## Implementation Decisions

### 后端

- **新模块 `src/backup.rs`**：承载导出/导入的类型与纯逻辑，路由层只做薄封装。便于对「解析 + 校验 + 事务性应用」单独单测。
  - 类型：`BackupFile`（`version`、`exportedAt`、`providers`、`virtualModels`、`apiKeys`、`settings`）。
  - `build_export(db)`：读全量 provider / provider_model / virtual_model / virtual_model_item / api_key / setting，组装为导出结构；provider 的 `api_key`、`extra` 与 api_key 的 `key` 解密（`crypto::decrypt`，失败回退空串——不可解的密文导出为空而非原文，避免再导入时二次加密脏数据）。
  - `parse_backup(bytes)` → 解析 + 结构校验，返回**精确错误**（带路径，如 `providers[2].models[0].providerModelId 缺失`）。
  - `apply_import(db, file)`：在**单事务**内整体替换——先删成员项 → 虚拟模型 → provider_model → provider → api_key（系统设置不删，见下），再按自然键逐层插入并建立旧→新 id 映射。任一步失败回滚。
  - 校验分两层：**结构级**（`validate_backup`，纯函数：自然键非空且唯一、成员引用可解析、枚举/数值范围、成员生效协议与虚拟模型接口类型匹配——生效协议 = 模型级 `protocolType` ?? 供应商 `protocolType`，Full Compatible 豁免）+ **值级**（路由层 `validate_import_values`，复用既有 helper：供应商字段/`validate_extra`/`validate_protocol_billing`/`validate_proxy`、设置 `validate_setting_value`，与各业务创建/更新接口同口径）。
  - **API Key 处理**：导出含明文 `key` + 元数据（`name`/`enable`）；导入时重新生成 `key_hash`（复用现有哈希函数）后加密入库。冲突键（唯一 `name`）在替换语义下由删除步骤消除。
  - **系统设置处理**：导出**全部** `setting` 行（含 language/timezone 等种子键——完整还原需要它们）；导入**只 upsert 覆盖**、不删除——系统设置的新版本新增键不应被旧备份清掉，备份里没有的键保留系统当前值。`value` 按备份原文写入，入库前复用设置声明类型校验（含 `max_consecutive_failures` ≥ 1、allowlist 黑名单等设置页同口径校验）。
- **新路由 `src/routes/backup.rs`**，挂载为 `.nest("/api/backup", backup::routes())`：
  - `GET /api/backup/export`：返回 `BackupFile` JSON（受会话保护，走统一 `Response<T>`）。文件名由前端生成。
  - `POST /api/backup/import`：body 为备份 JSON 原文；成功返回成功响应（data 含导入计数）；失败返回 400 + 具体错误消息（中文），供前端错误弹窗展示。
- **导出格式**（`version: 1`，字段 camelCase）：

```json
{
  "version": 1,
  "exportedAt": "2026-09-07T12:00:00Z",
  "providers": [{
    "name": "openai", "enable": true, "baseUrl": "https://api.openai.com/v1",
    "apiKey": "sk-plaintext", "customHeader": "{}", "protocolType": 0,
    "billingMode": 0, "extra": "{}", "sortOrder": 0,
    "proxyEnabled": false, "proxyAddr": "", "disabledReason": null,
    "models": [{
      "providerModelId": "gpt-4o", "contextLength": 128000, "maxOutputTokens": 4096,
      "reasoning": false, "toolUse": true, "imageUnderstand": true,
      "videoUnderstand": false, "protocolType": null,
      "proxyEnabled": false, "proxyAddr": ""
    }]
  }],
  "virtualModels": [{
    "displayId": "gpt-turbo", "enable": true, "loadBalancingStrategy": 0,
    "fallbackStrategy": 0, "interfaceType": 0,
    "items": [{
      "providerName": "openai", "providerModelId": "gpt-4o",
      "enable": true, "cascadeDisabled": false
    }]
  }]
}
```

  成员用**自然键** `(providerName, providerModelId)` 引用模型，跨机器/跨数据库稳定；导入时解析为新 id。`customHeader` / `extra` 保持字符串原样导出（与 API 入参口径一致）。

```json
  "apiKeys": [{
    "name": "itest-key", "key": "lg-plaintext", "enable": true
  }],
  "settings": [{
    "key": "site_title", "value": "…", "type": "String"
  }]
}
```

### 前端

- **入口**：设置页 `PageHeader` 修改密码按钮**左侧**新增图标按钮（`Download`/`DatabaseBackup` 风格图标），点击打开备份弹窗。
- **`BackupDialog`（备份弹窗）**：标题 + 两个按钮「导出备份」「恢复备份」+ 关闭。导出：`GET /api/backup/export` → 生成 Blob 下载（文件名如 `llm-gateway-backup-YYYY-MM-DD.json`）。恢复：关闭本弹窗 → 打开导入弹窗。
- **`ImportDialog`（导入弹窗）**：上半部分拖拽区（原生 `dragover`/`drop`，拖入后高亮并显示文件名）+ 下半部分「手动选择文件」按钮（隐藏 `<input type="file" accept=".json">`）+ 底部「确认导入」（未选文件时禁用）。点「确认导入」→ 先弹**破坏性确认**（明确文案：将清空并替换当前全部供应商/虚拟模型/API Key/系统设置；并提示备份文件含明文密钥请谨慎处理）→ 确认后读文件文本 `POST /api/backup/import`。
  - 成功：Toast「成功导入」、关闭弹窗、失效并刷新 providers / provider-models / virtual-models / api-keys / settings 查询。
  - 失败（HTTP 400 / 网络错）：打开**错误弹窗**展示后端返回的具体错误消息（+ 必要时的原始信息）。
- **i18n**：`zh-CN.ts` / `en.ts` 新增 `backup` 命名空间（入口、两按钮、拖拽区文案、选择文件、确认导入、破坏性确认文案、成功 Toast、错误弹窗标题/正文）。
- 不引入新依赖：拖拽用原生事件，弹窗用 shadcn Dialog / AlertDialog，Toast 用现有 `useToastActions`。

## Testing Decisions

好测试 = 只测外部行为，不测实现细节；复用仓库既有缝隙。

- **后端路由集成测试**（新增 `tests/backup_integration.rs`，走 `build_authed_app`，内存库 + 现有注入凭证）：
  - 导出：种子若干供应商/模型/虚拟模型/API Key/系统设置后 `GET /api/backup/export`，断言 JSON 含全部记录、`apiKey`/api key `key` 为明文（可用 `/api-key` 解密端点对照）、成员引用正确。
  - 导入（空库）：导出→清空→导入→断言数据完整重建（含 API Key 可鉴权、设置值恢复）。
  - 导入（已有数据）：先造数据再导入另一份备份，断言旧数据被整体替换、成员指向新插入的模型、API Key 重算哈希后可用。
  - 导入（系统设置）：备份里没有的设置键保留系统当前值不被删除；备份里有的键被覆盖。
  - 错误路径：非法 JSON、缺字段、`version` 不支持、成员引用不存在的模型 → 400 + 具体错误消息，且断言库中数据未变（事务回滚）。
- **后端纯函数单测**（`src/backup.rs` 内 `#[cfg(test)]`）：`parse_backup` 对各类畸形输入返回精确的错误路径与消息；`build_export` 组装正确；`apply_import` 的事务回滚（注入失败点）。
- **前端组件测试**（`web/src/components/__tests__/`，mock hooks / `ky`）：BackupDialog 点导出触发下载、点恢复切到 ImportDialog；ImportDialog 拖拽/选择文件显示文件名、确认导入流程 → 成功 Toast、失败弹错误弹窗。沿用现有 dialog 测试的 `act` + mock 模式。

## Out of Scope

- 不导出 cron 任务、会话、请求历史、登录用户（密码/会话是运行时身份，不属于可迁移配置）。
- 备份文件不加密、不设密码（明文密钥由用户自行保管文件）。
- 不做增量/差异恢复、不做导入前 dry-run 预览。
- 系统设置导入**不删除**当前键（只覆盖备份里出现的键），避免清掉新版本种子键。
- 不新增数据库表、不做 schema 迁移。
- 不引入前端拖拽库。

## Further Notes

- 数据库无外键约束（逻辑外键 + 级联硬删由代码保证），导入整体替换必须在**单个事务**内按依赖序删除与插入，避免中途失败留下孤儿行。
- 导出 `disabledReason` / `enable` / `cascadeDisabled` 等运行时状态，保证恢复后忠实还原启用/停用情况。
- 请求体上限 5MB（现有 `DefaultBodyLimit`）对配置 JSON 足够。
- 生产库 provider 的 `api_key`/`extra` 与 api_key 的 `key` 为加密存储，导出解密（失败回退空串）、导入重新加密（复用现有 `crypto`；`decrypt` 对无前缀的历史明文原样返回，天然兼容）。
- API Key 在设置页有独立管理区，导出/恢复后前端需刷新 api-keys 列表。
- 需求来源与已拍板决策见 `.scratch/backup-restore/REQUIREMENTS.md`。
