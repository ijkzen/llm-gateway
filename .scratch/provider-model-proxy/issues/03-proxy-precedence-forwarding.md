# 03: 转发/测速代理解析按「模型 → 供应商 → 直连」优先

**What to build:** 向模型上游发请求（/v1 转发与模型测速 test_model）时，代理解析为：模型开启代理且地址有效 → 用模型地址；否则供应商开启且地址有效 → 用供应商地址；否则直连。Member 仍只携带最终生效的代理（不带模型级字段）。upstream::call 的 CONNECT 隧道与连接池按代理隔离零改动。用量抓取/刷新模型列表路径不被模型级代理影响。

**Blocked by:** 01 provider_model 表新增模型级代理字段（迁移 19 + 实体）

**Status:** ready-for-agent

- [ ] resolve_proxy 纯函数实现优先级（模型开/关 × 供应商开/关 四象限），装配 Member 的两处（load_members 与 test_model）均改用它
- [ ] 纯逻辑单测覆盖四象限
- [ ] 代理连通集成测试（复用 spawn_connect_proxy）：模型开代理走模型地址（模型与供应商各配不同 mock 端口可区分）；模型关+供应商开回落供应商；都关直连
- [ ] 回归：模型代理不影响用量/刷新路径（既有 refresh 代理测试保持绿）
