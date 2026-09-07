# 04: SiliconFlow 用量真实验证收尾

**What to build:** 用真实 SiliconFlow 账号（用户提供 CookieCloud 配置与 X-Subject-Id）端到端验证：本地起服务、为 SiliconFlow (China) 供应商配好 extra 后，用量接口返回正确余额；同时验证历史回填对真实库生效。这是 01–03 测试覆盖不到的私有域接口的真实性兜底。

**Blocked by:** 01, 02, 03

**Status:** ready-for-agent

- [ ] 本地以测试密钥构建，建 SiliconFlow (China) 供应商（billing_mode=0）并填入 CookieCloud 配置 + x_subject_id
- [ ] `GET /api/providers/{id}/usage?refresh=1` 返回 kind=balance，含「账户余额」合计行（≈8.82 元级）与「认证奖励券」明细行
- [ ] 模板匹配接口（provider-templates/match）对 api.siliconflow.cn 返回含新键的模板
- [ ] 确认全量质量门（cargo fmt/clippy/test + pnpm lint/vitest）通过后，交付实现
