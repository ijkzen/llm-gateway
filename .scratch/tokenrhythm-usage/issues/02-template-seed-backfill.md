# 02: TokenRhythm 模板新建 + 历史回填

**What to build:** 新建 TokenRhythm provider 模板（seed alphabet T 段，base_url `https://tokenrhythm.studio/v1`，OpenAI Compat，billing_mode=0 按量，extra = CookieCloud 四键 + 用量开关），让用户能按模板创建并开启用量查询；并对生产已手动创建的 TokenRhythm Provider（base_url host `tokenrhythm.studio`）做每次启动幂等回填（只补缺 `cookie_cloud_server/uuid/password/domain` + `usage:true` + `usage_type:0`，不覆盖已设值）。

**Blocked by:** 01 TokenRhythm fetcher + host 分发

**Status:** ready-for-agent

- [ ] seed 新建 TokenRhythm 模板条目（总数 +1，插在 alphabet T 段）
- [ ] 新增启动回填 + host 谓词（只认 `tokenrhythm.studio`，大小写不敏感）
- [ ] 回填单测：历史 provider 补齐全部键 + usage:true，已设值不被覆盖，其它 host 不动
- [ ] seed/回填既有测试保持通过（upsert 计数 = TEMPLATES.len()）
- [ ] cargo test 全绿
