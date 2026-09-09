# FINDINGS · 06 usage 厂商抓取层审查（2026-09-10）

范围：`fetchers/` 全目录 13 文件（agentrouter/alibaba/api_key/balance/cloud_balance/copilot/krill/sensenova/siliconflow/stepfun/tokenrhythm/volcengine/xiaomi）+ `sensenova_login.rs`（676 行全读）/ `http.rs` / `cookiecloud.rs` / `volcengine_sign.rs` / `usage/mod.rs`（分发与写回）/ `error.rs` / `estimate.rs`（边界）+ 域内测试（各文件单测 ~70 例 + 集成覆盖盘点）。方法：共享/签名/登录文件会话内全读 + 13 fetcher 分两族（cookie/会话族 7 家、API-key/AK-SK 族 6 文件）由两个后台子代理逐行深读盘点（各行号断言已抽核磁盘复核：volcengine 回退/cloud_balance 分流顺序/alibaba sec_token/MiniMax 短路/sensenova 冷却与 invalid_grant 均亲验）→ 归位遗留项重估。清单模式：不改代码。**本票无 P1/P2**；两项归位遗留 + 一项冷却展示走拍板（见下）。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 06-01 | P3·需实证 | 逻辑/回退 | MiniMax 双端点首端点 401 短路：`ensure_not_auth_error?` 在循环内（api_key.rs:224-225）——首端点 401 直接 Auth 退出，注释承诺的 coding_plan 回退端点（:209-210）不会尝试 |
| 06-02 | P3·需实证 | 逻辑/回退 | 火山 Coding→AFP 回退被错误信封中断：非 auth 的 `ResponseMetadata.Error` 经 `auth_error?`（volcengine.rs:31）直接终局，与模块注释「失败且非鉴权错误时回退」（:3-5）相悖——无 Coding Plan 账号可能永远拿不到 AFP 窗口 |
| 06-03 | P3 | 逻辑/分类 | AK/SK 签名错误三处判 Auth 倒挂：body 含 `signature` → Auth（cloud_balance.rs:116/204、volcengine.rs:82）——时钟漂移/编码问题被当凭据失效误导用户换 key；而 `InvalidAccessKey`（火山两处谓词不含 accesskey）落 Upstream 反向漏判 |
| 06-04 | P3 | 逻辑/编码 | alibaba.rs:161-164 把 sec_token 裸拼 URL query 不做 percent-encode（对照 form 路径 :258-262 有编码）——token 含 `+`/`=`/`%` 时参数破坏或服务端误解 |
| 06-05 | P3 | 逻辑/自愈 | krill 会话失效自愈失效形态：code 仅按 i64 匹配（krill.rs:32-38，字符串 `"401"` 漏判）；200+HTML 登录页/挑战页走 Parse（:78-84）永不触发再登录——每 5 分钟固定失败直到用户手动 |
| 06-06 | P3 | 逻辑/顺序 | sensenova 冷却清除（sensenova.rs:144）早于 refresh_token 写回（:147-153）——写回失败（DB 错）时冷却已清，下轮无保护再登录 |
| 06-07 | P3 | 逻辑/分类 | cookiecloud.rs:51-56 非 200 全归 Upstream——server/uuid 配置错（404）按 502「服务器返回异常」呈现，用户可修正类应 400 |
| 06-08 | P3 | 测试覆盖 | 13 fetcher 的 HTTP 判定分支（401/403/3xx/200 包络→Auth 分流）零单测——所有判定逻辑在 async fetch 层，单测只覆盖纯解析/签名函数 |
| 06-09 | P3 | 测试覆盖 | 集成级真链路仅 6 条（DeepSeek/火山 billing/krill/sensenova/agentrouter/tokenrhythm）；copilot/siliconflow/stepfun/xiaomi/alibaba/api_key 全家/balance 三家/阿里 BSS 零集成覆盖 |
| 06-10 | P3 | 简洁/漂移 | 重复簇盘点（3xx-Auth 判定×5 份、浏览器 UA×2、round2×3、percent-encode×2、CookieCloud 拉取样板×4、snippet×2、krill/sensenova 登录状态机同构×2）——尊重「用量代码不抽重复」既有拍板，记录漂移风险；治理批顺势收敛最小集 |
| 06-11 | P3 | 可观测 | sensenova 冷却期合成 `Upstream(429)`（502 类）与冷却前 Auth(400) 跨周期交替呈现——用户看不出「密码错被锁」（归位二拍板：文案带剩余时间） |
| 归位一 | — | 会话失效分类 | 「CookieCloud 族 3xx=过期」在 reqwest 自动重定向下基本不可达；真分裂在 200 业务包络失效码。**已拍板：全面治理** |
| 归位二 | — | 登录冷却展示 | sensenova 冷却存在但隐藏；krill 无冷却。**已拍板：商汤文案带剩余时间；krill 保持现状** |

## 归位遗留项一：fetcher「会话失效」分类分歧重估【已拍板：全面治理】

### 分歧实态（13 fetcher 全量核对）

**共享判定**（http.rs:177-182 `ensure_not_auth_error`，只认 HTTP 401/403）——API-key/AK-SK 族 + copilot/sensenova：
api_key.rs×7 处、balance.rs×4、cloud_balance.rs×2、volcengine.rs、copilot.rs:25、sensenova.rs:175/216。

**内联 3xx+401/403 → Auth**（cookie/登录态族 6 家）：agentrouter.rs:50、tokenrhythm.rs:44、siliconflow.rs:41、stepfun.rs:59、xiaomi.rs:31、alibaba.rs:73-78。

**关键重估发现——3xx 分支基本不可达**：`UsageHttp::send`（http.rs:134-147）未禁用重定向，reqwest 默认自动跟随（≤10 跳）。会话过期的最典型形态「302 → 登录页」会被跟随到终点，fetcher 看到的是 **200 登录页/HTML**（→ Parse「响应不是合法 JSON」/「无法提取 sec_token」）而非 3xx（→ Auth）。6 家的 3xx→Auth 守卫只在「302 无 Location / 重定向超限 / 上游直接回 3xx 无 Location」等边角可达；xiaomi.rs:193-199 测试直接喂 302 `HttpReply` 断言 Auth，测的是分支逻辑本身而非真实链路。**「3xx=过期 vs 共享判定只认 401/403」的文字分歧近乎空转，真正分裂在 200 业务包络的失效码判定**：

| 家 | 200 包络中的「登录失效/key 无效」形态 | 现归类 | 测试锁定 |
| --- | --- | --- | --- |
| krill.rs:32-38 | `code:401`（i64） | Auth（触发再登录） | ✓ login_reason |
| sensenova.rs:178-184 | `400 invalid_grant` / 200 缺 access_token | Auth（触发登录） | 仅集成 |
| xiaomi.rs:8 注释 | `code:401`=登录态失效（注释自认） | **Upstream(200)**（:58-60/123-125） | ✗ |
| agentrouter.rs:62-64 | `success:false`+「登录状态已失效」 | **Upstream(200)** | ✓ 锁定 Upstream |
| tokenrhythm.rs:56-58 | `code:401`+「登录状态已失效」 | **Upstream(200)** | ✓ 锁定 Upstream |
| siliconflow.rs:53-55 | `code:40100`=unauthorized | **Upstream(200)** | ✓ 锁定 Upstream |
| moonshot balance.rs:99-101 | `code:401`+invalid key | **Upstream(200)** | ✓ 锁定 Upstream |
| zhipu/minimax/zenmux api_key.rs | `success:false` / `status_code:1004` | **Upstream(200)** | ✓ 锁定 Upstream |
| alibaba.rs:79-81 | sec_token 缺失（登录态失效典型） | Parse（文案含「登录态可能已失效」） | ✗ |

cookie 族五家 + 包络 API 三家无自愈能力（凭据用户侧维护），判 Auth 只影响 400「凭据无效或已过期」提示（引导用户重同步）vs 502 呈现——现状把**明示登录失效**的包络判成 502，用户看到「上游接口返回错误」而非「cookie 过期请重同步」。

### 拍板（2026-09-10）

**全面治理**，入实施批：
1. `UsageHttp` 禁自动跟随重定向（`Policy::none` 或 manual），使 3xx 真实可达；`ensure_not_auth_error` 扩展为含 3xx（或抽统一判定谓词供内联 6 家共用）。副作用核对：CookieCloud 服务器 http→https 跳转场景需回归；sensenova 登录走专用 `SensenovaLogin`（redirect limited 8）不受影响。
2. 按家补 200 业务包络 Auth 特征（上表 Upstream/Parse 行按各家实测特征归 Auth：xiaomi code=401、agentrouter/tokenrhythm 登录失效文案、siliconflow 40100、moonshot/zhipu/minimax/zenmux key 无效码）；**已锁定现状语义的测试（agentrouter/tokenrhythm/siliconflow/moonshot 四例）随批修改**。
3. 逐家特征需实测确认（每家的包络错误码形态），实施时按家排期、可分批。
4. krill code 支持字符串数字（`num` 同款）、sensenova invalid_grant 匹配去掉引号假设（`contains("\"invalid_grant\"")` → 宽松子串）随批精化。

## 归位遗留项二：登录冷却状态展示重估【已拍板：商汤文案带剩余时间；krill 保持现状】

- **现状**：sensenova 是唯一有冷却的 fetcher——15min 冷却（sensenova.rs:36-40，> 商汤 forbidLoginForMoment 约 10 分钟锁窗）、进程内静态表（:42-48）、冷却期合成 `Upstream(429, LOGIN_COOLDOWN_MSG)`（:116-127，`failed_secs_ago` 只进 tracing）、失败仅 Auth/Upstream 冷却（:132-141）、成功清除（:144）。krill 登录无任何冷却（错密码每 5 分钟撞一次登录接口）。
- **重估**：冷却机制本身合理且必须（撞锁会无限刷新锁定窗）。问题在展示：冷却期 502「上游接口返回错误 (HTTP 429)」与冷却前 Auth 400「凭据无效或已过期」跨周期交替，用户无法把「密码错→被锁→冷却中」连成因果；冷却状态无专门接口（详情页只能看到当轮错误）。
- **拍板（2026-09-10）**：① 冷却文案带剩余时间（如「账号被临时锁定，约 N 分钟后自动重试」，从冷却表时间戳算出），不引入新错误类别、不改 400/502 结构；② krill 保持现状不加冷却（自家站点、登录失败概率低、配错一次改对即可；若 krill 服务端出现锁定窗口再补），登记观察。③ sensenova 冷却清除顺序（06-06：清冷却在写回前）随批调整为写回成功后再清。

## 各条证据

### 06-01 MiniMax 双端点 401 短路（P3，需实证）

api_key.rs:209-210 注释「先 GET /v1/token_plan/remains，失败回退 …/coding_plan/remains」；实现 :224-225 在循环**内**对每个端点先 `ensure_not_auth_error(&reply)?`——首端点返回 HTTP 401/403 时整个 fetch 直接 `Err(Auth)` 退出，第二端点不试。回退只在「200 包络业务错误 / Parse 失败 / 非 401 非 200」时发生。若真实上游对「未订阅该 API」回 401（而非 404/200 包络），仅有 coding plan 的 MiniMax 账号会整轮 Auth、零窗口产出。**需实证**：真实端点对未订阅形态的返回码（2026-09-07 前后 MiniMax 接入的实测夹具可复现）。

### 06-02 火山 Coding→AFP 回退被错误信封中断（P3，需实证）

volcengine.rs 模块注释（:3-5）「失败且非鉴权错误时回退 GetAFPUsage」；实现（:27-35）：`parse_coding_plan` Err 后 `auth_error(&coding)?`——`auth_error`（:71-94）对 HTTP 200 + `ResponseMetadata.Error`（非 auth/signature/denied 三词）返回 `Err(Upstream)`，`?` 直接终局，**AFP 不试**。回退实际只在「200 无 Error 信封 + 形状不匹配（Parse）」时发生：错误信封=终局、形状不匹配=回退，与注释语义相悖。若无 Coding Plan 账号的服务端以错误信封返回（2026-09-07 排障记忆：权限缺失时报 permission denied 信封，且含 denied 词 → 直接 Auth），AFP-only 账号将永远拿不到 AFP 窗口。**需实证**：无 Coding Plan 账号 GetCodingPlanUsage 的真实响应形态；若为错误信封则修=错误信封也回退 AFP（非 auth 三词时）。

### 06-03 AK/SK 签名错误判 Auth 倒挂（P3，逻辑/分类）

三处把错误文本含 `signature` 归 `UsageError::Auth`（用户可见「凭据无效或已过期」）：cloud_balance.rs:116（阿里 BSS，`signature|accesskey|forbidden`）、cloud_balance.rs:204（火山费用中心，`auth|signature|denied`）、volcengine.rs:82（Coding/AFP，同三词）。问题双向：
- **SignatureDoesNotMatch → Auth**：真实根因常是本地时钟漂移、编码、content-type 与签名不一致（volcengine_sign.rs:15 与 http.rs:97 的 `application/json` 只靠注释互锁）——用户按提示换 key 问题依旧。属固有歧义（响应无法区分），建议 UI 侧提示「若 key 未变请检查系统时间/时区」（随实施批评估）。
- **InvalidAccessKey → Upstream**（火山两处谓词不含 accesskey，阿里侧含）：真实凭据错误反而不提示换 key。
- 附加：阿里 BSS 的 body 判定（:115-121）只在「HTTP 200 且无 Data」时执行，而 fetch 层 :66-68 对非 200（含 4xx 签名错）先转 Upstream——同一错误的分类随传输状态漂移（阿里 RPC 实际错误传输形态需实证，多数为 200 包络则影响小）。

### 06-04 alibaba sec_token 裸拼 URL（P3，编码边界）

alibaba.rs:161-164：`format!("{}/data/api.json?...&sec_token={}", ..., sec_token)` 直接插入 query，不做 percent-encode（region 为固定安全值）；对照 Token Plan 路径 :258-262 的 sec_token 走 form 时经 `form_encode_value` 编码——同一凭据两处处理不一致。sec_token 提取自首页 HTML（:92-107 引号截断），若为含 `+`/`=`/`%` 的 base64 形态，`+` 在 query 中=空格、`=` 截断参数。**需实证** token 字符集（现网响应样本）；修=统一 percent-encode。

### 06-05 krill 会话失效自愈失效形态（P3，逻辑/自愈）

krill.rs:32-38 `login_reason`：`code` 仅 `Value::as_i64` 匹配 401——上游若返回字符串 `"401"` 漏判（对照 fetchers/mod.rs `num` 已兼容字符串，此处未用）；:78-84：HTTP 200 + HTML（登录页/挑战页，jwt 过期的另一典型形态）→ `parse_subscription` 直接进 JSON 解析 → Parse——不触发再登录。两形态都使「jwt 失效→自动重新登录→写回」的自愈链（usage/mod.rs:73-87）失效，每 5 分钟固定失败直到用户手动。krill 是自家站点（krill-code.net），可用真实响应快速实证；修=code 支持字符串 + 200 非 JSON/HTML 判定为 Auth 候选。

### 06-06 sensenova 冷却清除早于写回（P3，顺序）

sensenova.rs:144 登录成功先 `login_failures().remove(...)` 清冷却，:147-153 才 `write_back_extra_key`——写回失败（DB 错误）传播 Err 时冷却已清，下一轮（5 分钟后）无冷却保护直接再登录；若此时商汤锁窗仍在（10 分钟 > 5 分钟周期），会撞锁刷新锁窗。修=写回成功后再清冷却（随归位二拍板③）。

### 06-07 CookieCloud 非 200 全归 Upstream（P3，分类）

cookiecloud.rs:51-56：`reply.status != 200` → `Upstream(status, "CookieCloud 服务器返回异常")`——server 地址错（DNS/连接失败走 Network 正确）、**uuid 错（404）**、服务器返回 5xx 都归 502「上游异常」；uuid 错属用户可修正配置（400 类 MissingCredential/文案「检查 uuid」更对症）。低危（502 呈现不阻塞，文案仍可见状态码）。修=404 归 MissingCredential/明确文案。

### 06-08 HTTP 判定分支零单测（P3，测试覆盖）

13 fetcher 的 `#[cfg(test)]` ~70 例几乎全部只测纯解析/签名函数（parse_*/extract_*/sign）；会话失效判定、3xx/包络分流、回退编排全在 async fetch 层，**零单测**。实例：api_key.rs 六家 fetch 的 401 映射/双端点回退/四请求编排（Command Code）零覆盖；copilot/agentrouter/stepfun/siliconflow/xiaomi 的 Auth 分支零覆盖；sensenova 的 invalid_grant→Auth/缺 access_token→Auth/冷却记录与清除/写回触发全无单测（仅集成间接覆盖）；krill 是唯一例外（login_reason 谓词有 4 例）。HTTP 层判定本可抽纯函数（如 `classify_auth(reply, body_features) -> UsageError`）后单测——随归位一治理批（统一判定谓词）自然获得测试面。

### 06-09 集成覆盖仅 6 条真链路（P3，测试覆盖）

tests/ 盘点：provider_usage_integration（DeepSeek 余额/krill/sensenova/agentrouter/tokenrhythm 的 E2E）+ lb_failure_disable_integration.rs:328-400（火山费用中心余额 0/非 0）+ usage 相关（缓存过期重取等）共 **6 条经 fetcher 的真链路**。零集成覆盖：api_key.rs 六家（opencode/kimi/zhipu/minimax/zenmux/commandcode）、balance 的 moonshot/openrouter/stepfun_account、cloud_balance 阿里 BSS、alibaba（cody/token 逆向）、xiaomi、siliconflow、stepfun、copilot。quota_gate/boundary_probe 集成直接种 usage cache 不经 fetcher。风险：逆向接口（alibaba/xiaomi/cookie 族）形态漂移最需要回归网，偏偏全裸。建议（实施批）：以 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 起每家一条最小 mock 链路（成功 + 失效两形态），优先 alibaba/cookie 族。

### 06-10 重复簇盘点（P3，简洁/漂移风险记录）

尊重既有拍板「用量查询代码一律不抽 round2/UA 等重复」，此处只记录跨文件重复簇与漂移实害（供治理批顺势收敛最小集）：
- **3xx-Auth 判定 ×5 份逐字重复**：agentrouter.rs:50 / tokenrhythm.rs:44 / siliconflow.rs:41 / stepfun.rs:59 / xiaomi.rs:31（alibaba.rs:73 同款）——归位一治理批统一为共享判定后自然消除。
- **浏览器 UA ×2**：agentrouter.rs:27-28 / tokenrhythm.rs:23-24 逐字重复。
- **round2 ×3**：agentrouter.rs:80-82 / tokenrhythm.rs:74-76 / siliconflow.rs:114-116。
- **percent-encode ×2**：cloud_balance.rs:91-101（阿里）+ volcengine_sign.rs:103-113 同语义。
- **CookieCloud 拉取样板 ×4**：alibaba/xiaomi/stepfun/tokenrhythm 各自 fetch_cookies+cookie_header 拼装（差异在域名与补充头，模板收敛面有限）。
- **snippet ×2**：fetchers/mod.rs:75-82 + sensenova_login.rs:491-498。
- **登录状态机同构 ×2**：krill（jwt 失效→登录→写回→重试一次）与 sensenova（refresh 失效/缺失→登录→写回→重试一次）——sensnover 多冷却+轮换，同构但差异点真实，不建议合并。
- 漂移实害示例：06-03 的三词谓词（阿里含 accesskey、两火山不含）即为逐份复制后各自演化的结果。

### 06-11 sensenova 冷却期 400/502 交替呈现（P3，可观测；归位二已拍板）

冷却期错误=`Upstream(429, LOGIN_COOLDOWN_MSG)`（sensenova.rs:116-127）属 502 类（error.rs:29-37 is_client_error false），与冷却前 Auth(400)「凭据无效」跨 usage_refresh 周期交替：用户先见 400 再连见 502，无法连成「密码错→被锁→冷却中」因果；`failed_secs_ago` 只进 tracing warn。**拍板（2026-09-10）**：文案带剩余冷却时间（从冷却表时间戳算 `remaining = 15min − elapsed`，如「商汤账号已被临时锁定，约 N 分钟后自动重试（请检查账号密码）」），保持错误结构不变。

## 已核验无问题区（避免后续票重复审查）

- **分发与域名防误配**：fetcher_for 按 host/path 分发（usage/mod.rs:171-218）有后缀嗅探单测锁定（agentrouter.org.evil.com / dashscope.aliyuncs.com.evil.com → false，:357-363）；火山/阶跃 path 分流测试锁定（:369-393）。
- **动态凭据写回单点**：krill jwt 与 sensenova refresh_token 写回均经 `write_back_extra_key`（usage/mod.rs:123-153）——重读最新行合并单键 + 严格解密失败 Err（不清空 extra）+ 加密落库；写回语义有单测（:410-449）。fetcher 层不直接写库（写回例外仅上述两处，均走单点）。
- **CookieCloud 解密**：EVP_BytesToKey(MD5,1 轮)+AES-256-CBC+PKCS7 与 easychen 协议一致（openssl 生成向量单测锁定，cookiecloud.rs:161-192）；Salted__ 头/长度校验、密码错→Auth、非 UTF-8→Parse 边界齐全；域名匹配规则（父域后缀可见性）与 Credentials::cookiecloud 非空校验（fetchers/mod.rs:63-71）正确；密码只参与本地密钥派生不出网。
- **sensenova_login**（自读 676 行）：JWE 5 段结构 + AAD=base64url(header) 参与认证（缺失即「密码错误」根因，解包还原测试双证 :622-675）、PKCE verifier/challenge 配对（:505-519）、账号锁定 forbidLoginForMoment 与凭据无效分流（:198-216，锁定返回可读 message）、六步登录逐级 warn+info 日志、redirect limited 8 + cookie store 只服务于登录链。此前两轮排障教训（AAD/PKCE）均已落地且有测试。
- **volcengine_sign**（自读+子代理逐段核对）：与 AWS SigV4 的差异（算法串无 AWS4 前缀/派生密钥首轮直接用 SK/scope 尾 `request`/`HMAC-SHA256` 头）全部为火山有意变体，模块头注释声明实测对齐；known-answer 测试与 python 独立对拍（:141-166）；GET/POST 签名分叉与消费方实际头一致。残余脆弱耦合（CONTENT_TYPE const 与 http.rs:97 注释互锁）已在 06-03 附注。
- **共享 http 层**：client 按 proxy 维度进程级缓存（P5 已修，http.rs:19-32）；15s 超时；LLM_GATEWAY_USAGE_HTTP_OVERRIDE 测试缝统一（usage 全部请求含登录链）；ensure_not_auth_error 单点（401/403）供 8 家使用——治理批的扩展点。
- **窗口/时区口径**：reset_ts 对 ISO/无时区（按设置表时区）/ms/sec/非正占位（-1/0→None）语义齐全且有测试（fetchers/mod.rs:92-177，NAIVE_TZ_LOCK 串行化）；各家秒级时间戳字符串（sensenova reset_secs）、ms 直读（command code resetAt）分别按实证口径处理。
- **模块边界**：fetchers 不感知缓存/门控（FetchOutput 纯产出，07 票域消费）；estimate 为无依赖纯核心（路由消费，不涉抓取层）；usage_rank（proxy）只读成功落库的 UsageData——抓取层错误分类不影响门控正确性（门控只在成功数据上运行），06 诸条均为可观测/自愈/UX 级。
- **测试隔离**：sensenova 冷却静态表有 reset_login_failures（:50-53）供集成测试隔离。

## 性能/内存轮结论

无实质问题。正向确认：reqwest client 进程级缓存（每 5 分钟全量刷新不再每轮重建握手，P5 修复在 http.rs:19-32 生效）；单家单轮请求数小（1-4 个）；alibaba 逆向链（CookieCloud+首页+RPC）每轮 3 请求属必要；sensenova 登录 client 每次 query_provider_usage 重建但仅登录路径使用（登录低频）；冷却表为进程内 HashMap 常量级。P3 观察：sensenova 冷却表无 in-flight 单飞——定时 usage_refresh（有 try_lock）与手动 ?refresh=1 并发时可能同时进登录（登录成功写回幂等、失败双记冷却，后果轻微）；krill 同理无锁（归位二已拍板保持）。下载/图片类无（该域在 05 已记）。
