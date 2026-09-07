# 01: SiliconFlow 用量 fetcher 与 host 分发

**What to build:** 新增 SiliconFlow（中国站）用量 fetcher：用 CookieCloud 凭据 + 用户填的 `x_subject_id` 拉取控制台钱包接口，归一化为余额型用量（合并「账户余额」primary 行 + 每券明细行），并把 `api.siliconflow.cn` 接入用量查询 host 分发。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 新增 fetcher：GET `https://cloud.siliconflow.cn/walletd-server/api/v1/subject/wallets?pageSize=50&visible=1`（不带 stage），请求头 `Cookie`（CookieCloud 按 `cloud.siliconflow.cn` 取）+ `X-Subject-Id`（取自 extra.x_subject_id）
- [ ] 金额换算：cap/used/balance 字符串，1 元 = 10¹² 单位，转元；不可解析条目跳过
- [ ] 归一化：余额型钱包（cap != -1 且 balance 可解析）每张一条明细（primary:false）；合计置顶一条「账户余额」（primary:true）；授信账户（cap=-1/无 balance）跳过
- [ ] 券名取多语言 name 的 zh-cn，解析失败回退 benefitId/walletId
- [ ] 鉴权失败（401/403/3xx）→ UsageError::Auth；code != 20000 / 缺 data.wallets → 明确错误
- [ ] `fetcher_for` 增加 `api.siliconflow.cn` → SiliconFlow 映射
- [ ] fetcher 解析纯函数单测（合并/跳过授信/单位/字符串数值/name 取 zh-cn/错误）
- [ ] 分发映射测试补 `api.siliconflow.cn` 断言
