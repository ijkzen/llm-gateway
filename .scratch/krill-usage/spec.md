# Spec — Krill 用量查询

Status: ready-for-agent

## Problem Statement

llm-gateway 目前无法查询 Krill 账户的余额或套餐额度。Krill 同一账户可以同时持有钱包余额和订阅套餐，但网关中的每个 Provider 已通过付费模式明确表达它应该作为按量成员还是订阅成员。管理员需要配置邮箱和密码后由系统自动维护 Krill JWT，并在详情页看到与 Provider 付费模式一致的用量；已有的 Krill Provider 也必须在升级后得到完整、加密且可用的配置，而不是只对新建数据生效。

## Solution

识别三条 Krill 模型 API 域名，并通过 Krill 控制台的 subscription API 拉取统一账户总览。查询时优先使用保存在 Provider extra 中的 JWT；仅在 JWT 缺失或认证失败时使用邮箱密码登录，立即加密回写新 JWT，再重试一次。返回内容严格由 Provider 的付费模式决定：按量模式展示钱包与福利组成的可用总余额及明细；订阅模式逐份展示 active 套餐的额度窗口。

为准确表达 Krill 日套餐，通用订阅窗口增加 Daily，并将 LB 比较链扩展为 5h、日、周、月。新增按量与订阅两个 Krill 模板；启动模板同步时对三条 Krill 域名的历史 Provider 执行幂等 extra 回填和 usage_type 校正。管理界面允许输入邮箱与密码，但隐藏由后端维护的 JWT，并确保编辑时不会丢失隐藏值。

## User Stories

1. 作为管理员，我希望从模板创建 Krill 按量 Provider，以便把钱包余额纳入按量负载均衡。
2. 作为管理员，我希望从模板创建 Krill 订阅 Provider，以便把套餐剩余额度纳入订阅负载均衡。
3. 作为管理员，我希望两个模板默认使用国内极速 API 地址，以便无需手工填写常用入口。
4. 作为使用海外直连或 CDN 地址的管理员，我希望系统仍能识别 Krill，以便这些入口也可查询用量。
5. 作为管理员，我希望只配置邮箱和密码，以便系统自动获得 JWT。
6. 作为管理员，我希望 JWT 在有效期内被直接复用，以便每次查用量不重复登录。
7. 作为管理员，我希望 JWT 认证失效后自动重新登录并重试，以便约七天的 token 到期不需要人工维护。
8. 作为管理员，我希望网络错误或 Krill 5xx 不触发重新登录，以便上游故障不会形成登录风暴。
9. 作为管理员，我希望新 JWT 立即加密回写并保留 extra 的其他字段，以便后续查询可复用且不会损坏配置。
10. 作为管理员，我希望 TOTP 账户得到明确失败提示，以便不会误以为系统支持无法自动完成的交互认证。
11. 作为按量 Provider 的管理员，我希望看到可用总余额、钱包余额和福利余额，以便了解可消费总额及组成。
12. 作为负载均衡器，我希望以钱包加福利的总额比较 Krill 按量成员，以便钱包为零但福利可用时不误剔除。
13. 作为订阅 Provider 的管理员，我希望只看到 active 套餐，以便冻结和过期套餐不干扰判断。
14. 作为拥有多个 active 套餐的管理员，我希望每份套餐分别展示且带套餐名，以便同周期额度不会互相覆盖。
15. 作为日套餐用户，我希望看到日额度而不是被标成 5 小时额度，以便界面语义正确。
16. 作为周套餐或月套餐用户，我希望看到相应周期的已用、总额和重置时间，以便判断剩余额度。
17. 作为计次套餐用户，我希望看到 5 小时、周和月请求额度，以便获得 Krill 提供的完整窗口信息。
18. 作为订阅负载均衡器，我希望按 5h、日、周、月比较剩余比例，以便短周期约束优先决定选路。
19. 作为额度门控逻辑，我希望 Daily 与现有窗口采用相同耗尽规则，以便任何已提供窗口耗尽时都能停止使用该成员。
20. 作为无 active 套餐的管理员，我希望详情显示暂无额度而不自动停用 Provider，以便缺少可判定数据时保持原状态。
21. 作为已有 Krill Provider 的管理员，我希望升级后自动补齐用量配置，以便不用删除重建 Provider。
22. 作为已有 Krill Provider 的管理员，我希望历史回填保留原付费模式和现有凭据，以便升级不改变业务意图。
23. 作为显式关闭用量查询的管理员，我希望历史回填保留 usage=false，以便升级不擅自开启我关闭的功能。
24. 作为安全管理员，我希望 email、password、jwt 随整个 extra 一起加密保存，以便敏感凭据不以明文落库。
25. 作为编辑已有 Krill Provider 的管理员，我希望隐藏 JWT 不因保存表单而消失，以便一次普通编辑不会迫使重新登录。
26. 作为旧缓存的使用者，我希望缺少 Daily 的历史 JSON 仍可读取，以便升级不破坏已有缓存。

## Implementation Decisions

- Krill Provider 由模型 API Base URL 的精确 host 集合识别：`api-slb.krill-ai.net`、`api.krill-ai.net`、`api.cdn-krill-ai.com`。不使用后缀模糊匹配。
- 控制台接口固定为 `https://www.krill-code.com`；模型 API 地址不拼接控制台路径。
- 用量查询入口把 Provider 的 `billing_mode` 传给 Krill fetcher。模式 0 只构造 balance，模式 1 只构造 quota；不根据上游响应自动改写模式。
- 认证状态机为：有 JWT则查询 subscription；无 JWT 则登录；查询结果仅在 HTTP 401/403 或可解析业务 code 401 时进入登录；登录成功后回写 token，再查询 subscription 一次；第二次无论何种错误都直接返回。
- 登录响应必须同时满足 HTTP 200、`success=true`、`code=0` 且存在非空 `data.token`。`requires_totp=true` 映射为认证失败，不增加 TOTP 流程。
- 复用现有 HTTP 客户端、代理配置和错误类型，不增加 HTTP 依赖、Krill 客户端接口或 token 缓存。
- JWT 回写采用现有动态凭据回写模式：按 provider id 重读最新行、严格解密、只合并 `jwt`、重新加密并更新。无法解密或写库失败时终止查询，防止丢失 email/password 等字段。
- 并发查询不增加锁。两个过期 JWT 查询可能分别登录并最后写入不同但有效的新 JWT；最后写入者生效。只有观测到 Krill 旧 JWT 会被即时吊销或登录限流后才引入每 Provider 锁。
- 按量输出三个余额项：主项金额为 credit 与 welfare 相加，币种 USD；另有钱包与福利明细。数值兼容 JSON 数字和数字字符串，缺失字段视为解析失败而不是默认零。
- `WindowKind` 增加 Daily。旧缓存通过现有 serde 枚举表示保持兼容，因为旧 JSON 不含新枚举值。
- 订阅只消费 `status=active` 的项目。每份套餐按 billing_type 产生窗口，label 为 `plan.name`；同一窗口类型允许多条。
- `usd_daily` 从 quota 的 used_usd 与 daily_limit_usd 构造 Daily；`usd_weekly` 从可用的周期已用/上限字段构造 Weekly；`usd_monthly` 从 total_used_usd 与 total_limit_usd 构造 Monthly。具体字段存在性以文档所列结构为准，缺失时跳过该窗口。
- `request_count` 从账户级 `request_count_quota` 的 used/limit 5h、weekly、monthly 字段构造三窗。该账户级对象只输出一次，并使用计次套餐名称作为 label，避免多个套餐导致重复三窗。
- 重置时间优先使用 quota.window_reset_at；没有独立 reset 时可使用 subscription_end_at/cycle_end 作为该额度周期的结束时间。非法时间不导致整个响应失败，只省略 resets_at。
- 无 active 套餐或没有可解析窗口时返回 quota 且 windows 为空；这会让现有可用性判定返回未知，不触发自动停用。
- 订阅 LB 比较链扩展为 5h、Daily、Weekly、Monthly；每类仍取最差剩余项。耗尽判断遍历所有可推导的窗口，因此自然包含 Daily 和多套餐。
- 前端用量类型联合与标签映射增加 daily；余额卡片沿用现有通用渲染，无 Krill 专属组件。
- Provider extra 表单引入最小元数据规则：`jwt` 是隐藏派生字段，`password` 使用密码输入；usage 与 usage_type 仍不可编辑。表单合并 payload 时保留隐藏字段。
- 两个 seed 模板名称必须唯一，Base URL 和协议相同，billing_mode 与 usage_type 分别为 0/0 与 1/1；extra 预置空 email/password/jwt 和 usage=true。
- 历史回填是启动时的幂等数据同步，不是 schema migration：当前 schema 无变化，不占用新迁移版本。它扫描三个精确 host，严格解密 extra，补缺 email/password/jwt，缺失 usage 才设 true，始终按 billing_mode 校正 usage_type；显式 usage=false 保持不变。
- 通用首次模板插入回填不足以处理同 host 双模板和三 host，因此 Krill 回填应是明确的幂等步骤，并在模板 upsert 的每次启动路径执行。

## Testing Decisions

- 测试只断言可观察行为：请求序列、HTTP 输出、归一化 UsageData、数据库最终密文内容、模板最终内容和界面可见/不可见控件；不锁定私有辅助函数拆分。
- 最高 seam 是从数据库 Provider 行调用公开的用量查询入口，并通过现有 HTTP override 指向本地 mock server。该 seam 覆盖有效 JWT 不登录、JWT 缺失登录、401/403/业务 401 重登、只重试一次、新 JWT 回写和 billing_mode 输出。
- 纯解析测试覆盖按量数字字符串、daily/weekly/monthly/request_count、多 active 套餐、忽略非 active 套餐、缺字段安全降级和业务错误。
- UsageData/排序现有 seam 增加 Daily：验证优先级 5h→日→周→月、同类多窗口取最差值以及 Daily=0 触发不可用。
- 模板测试沿用 provider_template 的内存数据库测试：验证双模板、同 host 匹配、三个 host 历史回填、两种 billing_mode、保留凭据和 usage=false、usage_type 校正、密文保存和重复运行稳定。
- 前端组件 seam 沿用现有 ProviderEditDialog 与 ProviderUsageCard 测试：验证 JWT 不渲染、password 为密码输入、隐藏 JWT 在提交值中保留、Daily 中文/英文标签和按量三项展示。
- 不新增 Krill 专属代理测试；现有 UsageHttp 代理透传测试作为前置保障，集成 seam 只验证 Krill 登录与查询复用传入的同一客户端路径。

## Out of Scope

- TOTP、邮箱验证码、OAuth、refresh token 及交互式认证。
- 主动解析 JWT exp、提前刷新、独立 token 缓存或数据库列。
- quota-summary、request-logs/stats、auth/me、endpoint-settings、交易流水、套餐购买或升级。
- 根据上游账户内容自动改变 Provider 付费模式。
- 展示冻结/过期套餐。
- 为未实测字段制造推测值；无法确认的窗口保持不可用。
- 新的第三方依赖、新的 Krill 客户端抽象或单实现 trait。

## Further Notes

- Krill subscription 元素结构来自前端代码反推，尚无真实有套餐响应样例；实现必须对字段缺失宽容，并由 mock fixture 固化当前契约而不是假装已实测。
- 金额以现有 `f64` UsageData 输出，符合当前系统的展示与排序模型；不扩展到精确十进制定价核算。
- 源 API 文档标明接口未获官方公开承诺，若将来契约变化，应优先更新 fetcher 解析与 fixture，不改变通用 UsageData 语义。
