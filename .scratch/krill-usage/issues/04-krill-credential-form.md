# 04: Krill 凭据表单

**What to build:** 管理员应用任一 Krill 模板后只需填写邮箱和密码即可启用用量查询；JWT 由后端维护，不在界面暴露，也不会在编辑保存时丢失。

**Blocked by:** 03: Krill 模板与历史回填.

**Status:** completed

- [x] Krill 模板表单展示 email 和 password，password 使用密码输入控件。
- [x] jwt 不渲染、不参与必填校验，email/password 在用量开启时必填。
- [x] 创建 payload 保留空 jwt 键，编辑 payload 合并并保留已有隐藏 JWT。
- [x] 用户修改 email/password 后仍保留旧 JWT，由后端认证失败路径负责换新。
- [x] 非 Krill 模板现有 extra 字段展示和校验行为不回归。
- [x] ProviderEditDialog 与 ProviderUsageCard 组件测试先红后绿，覆盖隐藏 JWT、密码输入、Daily 和三项余额展示。

## Completion notes

红测确认 password 仍是明文输入；最小字段元数据调整后，ProviderEditDialog 与 ProviderUsageCard 共 12 个测试及 TypeScript 检查通过。
