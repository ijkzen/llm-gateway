# 02: Qianwen Token Plan 模板种子 + 存量回填

**What to build:** 让新建与**历史上已创建**的该 host 供应商都能在 UI 里填写凭据。seed 新增 `Qianwen Token Plan` 模板（base_url `https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1`、protocol_type 0、billing_mode 1 订阅制、extra = `cookie_cloud_server/uuid/password/domain` 四键 + `usage:true` + `usage_type:1`），前端模板下拉经既有 `find_by_domain_all` 自动出现；新增 `is_qianwenai_host` 谓词与启动幂等回填 `backfill_qianwenai_provider_extra`（只补缺上述键，不覆盖已设值），注册进 `upsert_templates`——生产 #28（extra 为 `{}`）升级后编辑弹窗才渲染出可填字段。

**Blocked by:** 01 Token Plan 共享解析（含 CN/INTL 修复）+ qianwenai fetcher + 分发

**Status:** ready-for-agent

- [ ] seed 新增 `Qianwen Token Plan` 模板（billing_mode=1、extra 四键 + usage 标记）
- [ ] `is_qianwenai_host`（精确 host、大小写不敏感）+ `backfill_qianwenai_provider_extra` 注册进 `upsert_templates`
- [ ] 模板单测：seed 含该模板且形态正确（billing_mode=1、extra 键齐全）
- [ ] 回填单测：插入 extra=`{}` 的该 host provider → 回填补齐全部键 + `usage:true`，已设值不被覆盖
- [ ] 既有 seed/回填测试保持通过（upsert 计数更新为新总数）
- [ ] `cargo test --all-targets` 全绿
