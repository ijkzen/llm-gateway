# 03: AGENTS.md 同步降级语义（docs 提交）

**What to build:** AGENTS.md 中「failover 重试（408/429/500/502/503/529）」的既有描述与改动后现状不符，同步为「成员失败（本地或上游 >=400）即降级尝试下一成员，直到成功或全部成员尝试完」的全量语义。独立 docs: 提交。CONTEXT.md/ADR-0020 已核对兼容，无需修改。

**Blocked by:** 01（成员尝试循环去状态码白名单）——文档描述改动后现状

**Status:** ready-for-agent

- [ ] AGENTS.md 无残留状态码白名单描述，failover 语义与代码一致
- [ ] 以 docs: 前缀单独提交，不混入 feat/test 提交
