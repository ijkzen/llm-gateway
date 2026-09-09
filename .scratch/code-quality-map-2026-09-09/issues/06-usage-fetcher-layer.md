# 06 · usage 厂商抓取层审查

Type: task
Status: claimed
Blocked by: 01

## Question

对 usage 厂商抓取层做全量审查：`fetchers/` 全目录（agentrouter/alibaba/api_key/balance/cloud_balance/copilot/krill/sensenova/siliconflow/stepfun/tokenrhythm/volcengine/xiaomi）+ `sensenova_login.rs` / `http.rs` / `cookiecloud.rs` / `volcengine_sign.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：各家解析/签名/窗口口径（含 2026-09-07 前后多次修复的教训点不复查已修项）、CookieCloud 解密边界；
- 实现简洁：13 个 fetcher 的重复结构（既有拍板「用量代码不抽重复」——审查只记录结构性风险，不推翻该拍板）、共享 helper 缺失面；
- 测试覆盖：哪些 fetcher 无单测锁定（对照各家解析测试清单）；
- 模块间调用：与 07 持久化/门控、usage_rank、路由用量预估的边界是否合适。

**归位遗留项**：fetcher「会话失效」分类分歧（CookieCloud 族 3xx=过期 vs 共享判定只认 401/403）+ 隐藏登录冷却状态展示——在此给出重估结论。

产出 `.scratch/code-quality-map-2026-09-09/findings/06-usage-fetcher-layer.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/06-usage-fetcher-layer.md`——11 条全 P3 无 P1/P2 + 两项归位遗留重估。方法=共享/签名/登录文件（http/cookiecloud/volcengine_sign/sensenova_login/usage/mod）会话内全读 + 13 fetcher 分两族由两个后台子代理逐行深读盘点（cookie/会话族 7 家 + API-key/AK-SK 族 6 文件），关键断言已逐条磁盘抽核（volcengine 回退/MiniMax 短路/sec_token 编码/冷却顺序/invalid_grant 亲验通过）。核心结论：

- **会话失效分类分歧（归位遗留一）重估出真问题位置**：「CookieCloud 族 3xx=过期 vs 共享判定只认 401/403」的文字分歧近乎空转——UsageHttp 未禁重定向（http.rs:134-147），reqwest 默认跟随 ≤10 跳，302→登录页被跟随成 200 登录页/HTML → Parse；6 家内联 3xx→Auth 守卫真实链路基本不可达（xiaomi 测试直接喂 302 HttpReply，测分支而非真链路）。**真分裂在 200 业务包络**：xiaomi 注释自认 code 401=登录态失效但代码落 Upstream，agentrouter/tokenrhythm/siliconflow/moonshot 的明示失效码被测试锁定为 Upstream(502)，krill code 仅 i64 匹配、200+HTML 走 Parse 永不触发再登录。**已拍板：全面治理**（UsageHttp 禁重定向 + 判定统一含 3xx + 按家补包络 Auth 特征，四例已锁定测试随批修改，逐家实证排期）。
- **登录冷却（归位遗留二）**：sensenova 15min 冷却机制合理且必须，但冷却期合成 Upstream(429) 与冷却前 Auth(400) 跨周期交替，用户看不出「密码错被锁」。**已拍板：商汤冷却文案带剩余时间；krill 不加冷却保持现状（自家站点，若出现锁窗再补）**；sensenova 冷却清除顺序（写回前清除，06-06）随批调整。
- **单家级新发现**：MiniMax 首端点 401 短路不回退第二端点（06-01）、火山 Coding 错误信封中断 AFP 回退与注释相悖（06-02，均需实证服务端错误形态）、AK/SK 签名错误三处判 Auth 倒挂（SignatureDoesNotMatch→Auth 误导换 key、InvalidAccessKey→Upstream 反向漏判，06-03）、alibaba sec_token 裸拼 URL（06-04）、krill 自愈失效形态（06-05）、CookieCloud 404 归 502（06-07）。
- **测试面**：HTTP 判定分支零单测（13 fetcher 只测纯解析/签名函数，06-08）；集成真链路仅 6 条，api_key 六家/balance 三家/阿里 BSS/alibaba/xiaomi/siliconflow/stepfun/copilot 零集成覆盖（06-09）——逆向接口最需要回归网却全裸。
- **重复簇**（06-10）：3xx 判定×5、UA×2、round2×3、percent-encode×2、cookie 样板×4——尊重「不抽重复」拍板只记漂移风险，治理批顺势收敛。

**需拍板问题**：已全部当场拍板（归位一全面治理、归位二商汤文案带剩余时间/krill 保持现状），无遗留。

Status: resolved
