# 04: 前端模型详情支持代理配置与展示

**What to build:** 模型详情弹窗「编辑」表单增加网络代理开关与地址输入（复用供应商代理的校验、文案、placeholder）；只读视图增加「网络代理」展示行（开=绿徽标+地址，关=灰徽标）。类型（ProviderModel/ProviderModelPayload）带新字段。添加模型弹窗不改（默认关闭，添加后编辑）。i18n 复用 providers.proxy* 文案，不新增 key。

**Blocked by:** 02 模型 CRUD/校验支持代理字段（路由层）

**Status:** ready-for-agent

- [ ] ProviderModel 接口与 payload 含 proxyEnabled/proxyAddr
- [ ] 编辑表单：开关 + 条件显示地址输入，superRefine 校验开启必填 + http:// 前缀；提交时关闭则清空地址
- [ ] 只读态展示「网络代理」行（开关状态徽标 + 地址），与 ProviderDetail 同款
- [ ] 前端测试覆盖：只读行展示、编辑保存代理字段、开启但地址空/非 http:// 显示校验错误
