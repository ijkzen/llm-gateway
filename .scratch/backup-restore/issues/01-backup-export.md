# 01: 后端备份导出

**What to build:** 提供导出端点，从数据库读全量配置组装为 versioned JSON 并返回：全部供应商（含解密后的明文 api_key / extra）及各自名下全部模型、全部虚拟模型及各自成员、全部 API Key（含明文 key）、全部系统设置。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] `GET /api/backup/export` 受会话保护，返回统一成功响应
- [ ] JSON 结构遵循 spec：`version: 1` + `exportedAt` + `providers`（含 `models`）+ `virtualModels`（含 `items`，成员以 `providerName`+`providerModelId` 自然键引用）+ `apiKeys` + `settings`
- [ ] 供应商 api_key / extra 与 API Key 的 key 导出为明文（复用 `decrypt_or_passthrough`，失败回退空串）
- [ ] 全量读取、无分页遗漏
- [ ] 纯函数 `build_export` 单测：组装正确、解密正确
- [ ] 集成测试：种子供应商/模型/虚拟模型/API Key/系统设置后导出，断言全含、密钥明文、成员引用正确

**Blocked by:** None
