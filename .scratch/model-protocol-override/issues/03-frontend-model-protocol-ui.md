# 03: 前端模型详情弹窗协议选择

**What to build:** 供应商模型详情弹窗支持为单个模型选择协议：编辑态提供下拉（「跟随供应商」+ OpenAI Compatible / OpenAI Responses / Anthropic Messages / Gemini 四种），保存后接口携带 `protocolType`（null=跟随供应商）；只读态显示该模型**生效**的协议与来源（自定义=协议名，跟随=「跟随供应商（供应商协议名）」）。刷新页面后选择保持。

**Blocked by:** 01（模型协议字段存储与 CRUD——弹窗需 API 回显与提交该字段）；与 02 互相独立可并行

**Status:** ready-for-agent

- [ ] `ProviderModel` 接口与提交 payload 增加 `protocolType: number | null`
- [ ] 编辑表单 schema 增加 `protocolType`（默认 null），下拉选项 =「跟随供应商」+ 复用 `PROTOCOL_TYPES` 四种协议
- [ ] i18n 新增「跟随供应商」文案键
- [ ] 只读态展示生效协议与来源（跟随供应商时带出供应商协议名，弹窗接收 `providerProtocolType` prop）
- [ ] 提交 `protocolType` 落库；更新后回到只读态显示新值
- [ ] 前端构建/类型检查/lint 通过（本 ticket 不新增组件测试）