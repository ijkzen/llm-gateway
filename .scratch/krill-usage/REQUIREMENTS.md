# REQUIREMENTS — Krill 用量查询

来源：用户请求「根据 `/Users/ijkzen/krill-api-docs.md` 接入 Krill 余额查询，同时支持订阅制和按量付费，处理历史数据，并新增两个 seed 模板」；接口事实以该文档 2026-09-03 的抓包与复现结果为准。

## 领域口径

- **Krill Provider**：模型 API Base URL 的 host 为 `api-slb.krill-ai.net`、`api.krill-ai.net` 或 `api.cdn-krill-ai.com` 的 provider。
- **展示模式**：只由 `provider.billing_mode` 决定；`0` 返回按量余额，`1` 返回订阅额度。上游账号同时拥有钱包和套餐时也不自动猜测或切换模式。
- **Krill 凭据**：`provider.extra` 中的 `email`、`password`、`jwt`。JWT 是后端维护的派生凭据，email/password 是 JWT 无效时重新登录的源凭据。
- **有效套餐**：`subscriptions[]` 中 `status == "active"` 的元素。冻结或过期套餐不展示、不参与门控或排序。
- **主余额**：`credit_balance_usd + welfare_balance_usd`，用于按量 Provider 的 LB 排序和耗尽判断；钱包与福利同时作为非主明细展示。

## 范围

1. **Krill fetcher 与认证恢复**
   - 用量查询固定请求 `https://www.krill-code.com/api/subscription`，Bearer JWT 认证；模型 API Base URL 只用于识别 Krill Provider。
   - `extra.jwt` 非空时先直接查询；JWT 缺失时用 email/password 登录。
   - 只有 subscription 请求返回 HTTP 401/403 或合法响应的业务 `code == 401` 时，才调用 `POST https://www.krill-code.com/api/auth/login` 获取新 JWT，并重试 subscription 一次。
   - 网络失败、5xx、非认证业务错误、JSON/字段解析错误不触发登录；一次查询最多登录一次、重试一次。
   - 登录成功后先将新 JWT 合并回写到加密的 `provider.extra`，保留其他字段，再用新 JWT 重试。写回失败则本次查询失败。
   - 登录要求 email/password；TOTP 响应明确返回认证不受支持，不尝试 TOTP 或其他交互式登录。
   - 继续沿用 provider 级 HTTP 代理，登录和 subscription 请求必须使用同一个 `UsageHttp`。

2. **按 `provider.billing_mode` 归一化**
   - 按量（0）：返回 `UsageKind::Balance`，三项依次为「可用总余额」（credit+welfare，`primary=true`）、「钱包余额」（credit）、「福利余额」（welfare），币种 USD。
   - 订阅（1）：返回 `UsageKind::Quota`，只处理 active 套餐；多份 active 套餐逐份输出，窗口 `label` 使用套餐名，以免同类窗口相互覆盖。
   - `usd_daily` 映射 Daily；`usd_weekly` 映射 Weekly；`usd_monthly` 映射 Monthly；`request_count` 使用账户级 `request_count_quota` 的 5h/Weekly/Monthly 三窗。
   - 金额套餐优先使用相应周期的 `used/limit` 字段，重置时间使用该周期可用的 reset/end 字段；缺字段的窗口保持不可用，不用 0 猜值。
   - 无 active 套餐或无法解析出任何窗口时返回 quota 空数据，不据此自动停用 Provider。

3. **Daily 窗口贯穿现有订阅能力**
   - `WindowKind` 新增 Daily，前端详情页展示「日额度」。
   - 订阅制 LB 比较顺序为 5h → Daily → Weekly → Monthly；同类多窗口继续取剩余百分比最差的一项。
   - Daily 与其他可用窗口一样参与额度耗尽判断；任一已提供窗口剩余为 0，沿用现有自动停用/剔除口径。
   - 旧缓存不含 Daily 时仍可反序列化和正常展示。

4. **Provider 表单与敏感字段**
   - 两个 Krill 模板的 extra 都包含 `email`、`password`、`jwt`、`usage`、`usage_type`。
   - 编辑弹窗只展示 email 和 password；password 使用密码输入；JWT 不展示、不要求用户填写，由后端获取和维护。
   - 开启用量查询时 email/password 必填，jwt 可空。
   - 前端更新 Provider 时必须保留已有隐藏 JWT；用户修改 email 或 password 后，旧 JWT 可以继续首次尝试，认证失败后按新凭据换取并回写。

5. **Seed 模板**
   - 新增 `Krill（按量付费）`：Base URL `https://api-slb.krill-ai.net/v1`，OpenAI Compatible，`billing_mode=0`，`usage_type=0`。
   - 新增 `Krill（订阅制）`：同一 Base URL 与协议，`billing_mode=1`，`usage_type=1`。
   - 同 host 模板匹配时两项都返回，由用户根据账户用途选择。

6. **历史数据幂等回填**
   - 启动时识别三个 Krill 模型 API host 下的既有 Provider，保留现有 `billing_mode`。
   - 为 extra 补齐缺失的 `email`、`password`、`jwt`；缺失 `usage` 时设为 `true`；`usage_type` 始终按现有 `billing_mode` 校正为 0/1。
   - 只补缺 email/password/jwt，不覆盖已有凭据；显式 `usage=false` 保持关闭。
   - 回填后的 extra 继续 AES-256-GCM 加密保存；解密失败必须报错或记录明确错误，不得清空或覆盖原 extra。
   - 回填可重复执行且结果稳定，覆盖三个 host，不依赖某一 seed 模板是否首次插入。

7. **测试与验证**
   - fetcher 纯解析测试覆盖按量、四类订阅、多 active 套餐、无 active 套餐和错误响应。
   - HTTP 集成测试覆盖：有效 JWT 不登录；JWT 空先登录；401/403/业务 401 登录并仅重试一次；网络/5xx/解析错误不登录；新 JWT 加密回写且保留 extra；代理透传路径不回归。
   - 历史回填测试覆盖三个 host、两种 billing_mode、保留显式 usage=false、保留既有凭据、幂等执行和密文保存。
   - seed、host 分发、Daily 序列化/排序/门控、Provider 表单与用量卡片均有回归测试。

## 非目标

- 不实现 TOTP、邮箱验证码、OAuth 登录或 refresh token。
- 不调用 quota-summary、request-logs/stats、auth/me、endpoint-settings 等非用量必需接口。
- 不根据 Krill 上游实际账户内容自动改写 `provider.billing_mode`。
- 不在 JWT 到期前主动刷新，不解析 JWT `exp`，不增加独立 token 缓存或新数据库列。
- 不展示冻结/过期套餐，也不实现套餐购买、升级或流水查询。
- 不承诺文档未实测的 subscription 字段一定存在；解析器对缺字段安全降级为不可用窗口。

## 用户拍板记录

- 新增 Daily，不把日额度伪装为 5h。
- 多 active 套餐逐套餐展示，窗口 label 为套餐名。
- 按量以钱包+福利总额作为 primary，同时显示两项明细。
- 仅认证失败自动重登一次。
- LB 比较优先级为 5h→日→周→月。
- JWT 在表单中隐藏，由后端自动获取和回写。
- 两个模板默认使用国内极速 Base URL，并兼容三条 Krill API host。
- 历史三 host Provider 自动启用缺失的 usage 标记并补齐凭据字段，billing_mode 保持不变。
