# 02: SiliconFlow 模板种子升级与历史回填

**What to build:** 把「SiliconFlow (China)」模板 extra 从 `{}` 升级为含 CookieCloud 凭据键 + `x_subject_id` + 用量开关的结构，并让已存在的 SiliconFlow (China) 历史供应商在启动时幂等补齐缺失键（只补缺、不覆盖），用户无需手动改 JSON。

**Blocked by:** 01 (需要 `is_siliconflow_host` host 判定，回填 host 谓词引用它)

**Status:** ready-for-agent

- [ ] seed「SiliconFlow (China)」extra 升级：`{"cookie_cloud_server":"","uuid":"","password":"","domain":"","x_subject_id":"","usage":true,"usage_type":0}`
- [ ] 国际站「SiliconFlow」(api.siliconflow.com) 模板保持 `{}` 不变
- [ ] provider_template 新增 `is_siliconflow_host`（host == api.siliconflow.cn），供回填用
- [ ] 新增启动幂等回填 `backfill_siliconflow_provider_extra`（复用 backfill_host_extras 管线）：host 命中的历史 provider 只补缺 5 个凭据键 + usage/usage_type，不覆盖已设值
- [ ] `upsert_templates` 中与 Krill/SenseNova 并列调用该回填
- [ ] 模板种子测试：SiliconFlow (China) 含全部新键；国际站不变
- [ ] 回填测试（仿 SenseNova/Krill 先例）：幂等、保留用户已设值、未知键保留、无关 host 不动、非法 extra 跳过不阻塞
