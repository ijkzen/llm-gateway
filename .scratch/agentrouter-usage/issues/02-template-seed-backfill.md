# 02: AgentRouter 模板 extra 升级 + 历史回填

**What to build:** AgentRouter 模板（seed 既有条目，host `agentrouter.org`）的 extra 从 `{}` 升级为 CookieCloud 凭据结构 + `new_api_user` + 用量开关，让新建 Provider 能直接开启用量查询；并对历史上已创建的 AgentRouter Provider 做启动幂等回填（只补缺 `cookie_cloud_server/uuid/password/domain/new_api_user` + `usage:true` + `usage_type:0`，不覆盖已设值）。

**Blocked by:** 01 AgentRouter fetcher + host 分发

**Status:** ready-for-agent

- [ ] seed 模板 AgentRouter 条目 extra 升级（条目数不变）
- [ ] 新增启动回填 + host 谓词（只认 `agentrouter.org`，大小写不敏感）
- [ ] 回填单测：extra=`{}` 历史 provider 补齐全部键 + usage:true，已设值不被覆盖
- [ ] seed/回填既有测试保持通过（upsert 计数不变）
- [ ] cargo test 全绿
