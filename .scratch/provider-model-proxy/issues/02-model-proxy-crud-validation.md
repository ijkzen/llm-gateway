# 02: 模型 CRUD/校验支持代理字段（路由层）

**What to build:** 供应商模型的创建、批量创建、更新接口接受并返回 `proxyEnabled`/`proxyAddr`，校验规则与供应商代理完全一致（开启时地址必填、`http://` 开头、无认证，非法返回 400 双语错误）。列表与详情响应带上两个新字段。添加时前端不传则默认关闭（serde default），不影响既有客户端。刷新模型列表仍只读供应商代理（不变）。

**Blocked by:** 01 provider_model 表新增模型级代理字段（迁移 19 + 实体）

**Status:** ready-for-agent

- [ ] 模型 create/batch/update 请求结构含 `proxyEnabled`/`proxyAddr`（默认 false/""），响应结构含同名字段（camelCase）
- [ ] 校验复用供应商的 validate_proxy（改 pub(crate) 可见性即可），create/batch/update 均生效
- [ ] 集成测试：create/batch/update 写读代理字段；开启无地址/非 http:// 前缀/含 @ → 400
- [ ] 刷新模型列表路径仍只读 provider.proxy_enabled/proxy_addr（零改动，回归测试保持绿）
