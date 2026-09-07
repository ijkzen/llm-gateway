# 01: 接口类型字段 + 迁移回填 + 类型路由与 /v1/models 过滤

**What to build:** 管理员能看到每个虚拟模型的接口类型（OpenAI Compatible / Responses / Messages / Gemini 保留 / Full Compatible），创建新虚拟模型默认 OpenAI Compatible，历史虚拟模型自动归为 Full Compatible；/v1/models 列表与单查只返回/匹配 OpenAI Compatible 与 Full Compatible 类型的启用模型；/v1/chat/completions 拒绝 Responses/Messages/Gemini 类型的虚拟模型（OpenAI 原生错误格式），类型不匹配的模型如同不存在。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] 迁移从 16 号段起编：virtual_model 新增接口类型整数列，枚举 0=OpenAI Compatible（新行默认）/1=Responses/2=Messages/3=Gemini（保留）/4=Full Compatible，存量行回填 4
- [ ] 虚拟模型 CRUD（创建/更新/列表/详情）携带接口类型；类型值域校验
- [ ] /v1/models 列表与 /v1/models/{display_id} 单查只返回/匹配类型 ∈ {0, 4} 且启用
- [ ] /v1/chat/completions 命中类型 1/2/3 的虚拟模型时返回模型不存在类 OpenAI 错误；类型 0/4 行为不变
- [ ] 集成测试：历史回填、/v1/models 过滤、类型路由拒绝（缝 B + 缝 A）
