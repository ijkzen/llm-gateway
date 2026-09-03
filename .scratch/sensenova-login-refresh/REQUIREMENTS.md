# REQUIREMENTS — SenseNova 账号密码登录自愈（refresh_token 失效回退）

来源：用户请求。当 `sensenova` 供应商用量抓取所用的 refresh_token 失效（退出登录/换密码/轮换断链）
导致用量获取失败时，用 `extra` 新增的 `username`/`password` 登录商汤控制台换取新 refresh_token 并回写数据库，
用量获取优先用 refresh_token，失败才走登录。登录接口形态见 `.scratch/sensenova-login-refresh/research.md`
（已实测：纯 API 可自动化，无验证码/人工 consent）。

## 范围

1. **凭据**：`extra` 新增 `username`（商汤控制台自定义用户名，6-24 位字母数字）与 `password`。
   - `refresh_token` 升级为**后端派生隐藏字段**（等同 Krill `jwt` 待遇）：前端不展示、后端校验豁免、
     模板保留空默认、登录后由后端写回。
2. **sensenova fetcher 状态机**（仿 Krill 模式）：
   - `extra_str("refresh_token")` 非空 → 现有续期（`grant_type=refresh_token`）+ pool-usage 查询；
     成功解析返回，轮换出的新 refresh_token 走现有写回。
   - 仅当 refresh_token **缺失**或续期**明确鉴权失败**（HTTP 401/403，或 200 响应 `{"error":"invalid_grant"}` /
     缺 `access_token`）→ 用 `username`/`password` 执行登录换取新 refresh_token。
   - 登录成功 → 新 refresh_token **先写回** provider extra（重读最新行、解密、只合并该键、重加密更新）→
     用新 refresh_token 重试一次续期+查询。
   - 网络错误 / 5xx / 解析错误**不触发**登录；登录失败（HTTP 400 `incorrectUsernameOrPassword`、
     HTTP 401/403、响应无 redirect）→ `UsageError::Auth`（沿用「用量查询凭据无效或已过期」链路）。
   - 登录只尝试一次，重试最多一次。
3. **登录实现**（纯 API，复用 research.md 六步）：authorize 拿 challenge → JWKS 取 RSA 公钥 →
   密码 5 段 JWE（RSA-OAEP + A256GCM，tag 独立第 5 段）→ `nova/login` → 跟 redirect（consent 自动）拿 code →
   `oauth2/token` 换 refresh_token。
   - Rust 需新增 `rsa` crate（RSA-OAEP）；aes-gcm/sha1/base64/rand 已有。
4. **模板种子**：SenseNova 模板 extra 改为 `{"refresh_token":"", "username":"", "password":"", "usage":true, "usage_type":1}`；
   存量 provider 靠既有模板 backfill 幂等合并补入 `username`/`password`。
5. **路由校验**：`validate_extra` 豁免清单加 `refresh_token`（后端派生，可为空，同 `jwt`）。
6. **前端**：`editableExtraKeys` 排除 `refresh_token`（同 `jwt`），弹窗只显示 username/password（password 掩码输入）。

## 非目标（ponytail 修剪）

- 不做 access_token 二级缓存（每次 fetch 直接续期，沿用现状）。
- 不做 TOTP / 短信验证码 / 图形验证码自动处理（本链路实测无验证码；若未来触发 captcha 则报 Auth 让用户处理）。
- 不改 `UsageError` 变体、不加新错误文案。
- 不做数据库迁移（extra 自由 JSON，模板 backfill 幂等合并即可）。
- 登录频率不做额外节流（沿用「仅明确鉴权失败才登录」门控，天然低频）。

## 用户拍板记录

- 仿 Krill 凭据策略：password 必填、refresh_token 隐藏为后端派生字段。
- 登录接口形态未知 → 先调研（已完成，见 research.md）。
- 新建 worktree、以 main 最新提交为基线独立分支实现。
