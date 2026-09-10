# FINDINGS · 21 FE 会话与演示域审查（2026-09-10）

范围：`pages/login.tsx`（258 全读）/ `pages/chat.tsx`（387 全读）/ `components/require-auth.tsx`（31 全读）+ hooks（use-auth 90 / use-init-settings 49 / use-locale 76 全读）+ 域内测试盘点（login-page 5 例/chat-page 6 例/require-auth 组件级）+ 交叉核对（tests/chat_integration.rs 后端契约、use-provider-models 类型、argon2 时序）。方法：域小（891 行）主代理直接全读，无子代理。清单模式：不改代码。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 21-01 | P2 | 逻辑/竞态 | 初始化流程的时区/语言保存与 init 提交赛跑：`saveInitSettings` 在 mutation 启动即同步发 PUT，而 init 端点要 argon2 哈希（数百 ms）后才建会话种 cookie——PUT 必然先到、401 被拒，用户首启选的时区静默丢失（落回种子默认），成功路径上还弹「保存失败」toast |
| 21-02 | P3 | 逻辑/契约 | chat.tsx 静默吞 03-01 的流内 error 帧：`{"error":...}` 无 choices → `deltaOf` 返回 {} 忽略 → 流中错误表现为内容戛然而止，无错误标记（03 票后端已发 error 帧+[DONE]） |
| 21-03 | P3 | 健壮/契约 | `eventData` 只取事件内首个 `data:` 行（SSE 规范允许多行 data 拼块；网关当前单行输出故不可达，契约脆弱记观察） |
| 21-04 | P3 | 测试覆盖 | 缺口族：21-01 无回归网（saveInitSettings 时序）、use-auth 三 mutation 零直测、chat 流内 error 帧/畸形 chunk 分支未测、use-init-settings 失败聚合零直测 |

**本票无需拍板项**。补充登记：login 初始化表单的 zod 长度校验按 UTF-16 code unit、后端按字节——与 18-15（ChangePasswordDialog）同族同根因，不重复编号。

## 各条证据

### 21-01 初始化保存与 init 提交赛跑（P2，竞态）

login.tsx:178-187（InitForm.handleSubmit）：`onSubmit(values)`（启动 init mutation）后**同步** `void saveInitSettings(locale, timezone)`——两个 PUT 立即发出。而 init 端点要 `hash_password`（argon2，数百 ms）成功后才建会话、Set-Cookie（routes/auth.rs:114-138）。settings PUT 在会话 cookie 存在之前到达 → 中间件 401（401 不跳转靠 onLoginPage 白名单兜底=正确）→ saveInitSettings 记入 failed → login.tsx:182-186 弹「保存失败」toast。净效果：**每次首启初始化，语言与时区两个 PUT 几乎必然失败**（argon2 延迟保证 PUT 先败），用户选的时区静默丢失落回种子默认 Asia/Shanghai，成功初始化还伴随一条错误 toast。注释（:180-181）只意识到「init 失败时 PUT 被拒」的半面，没意识到成功路径同样必败。默认解：saveInitSettings 移到 mutation onSuccess（会话已建立）；顺带把「语言已在顶部切换入口同步」的注释改为真实时序。回归测试归 21-04。

### 21-02 chat 静默吞流内 error 帧（P3，契约）

chat.tsx:35-52 `deltaOf`：`JSON.parse(data)` 后取 `choices[0].delta`——03-01 拍板后泵对转换失败发 `{"error":{...}}` 帧+[DONE]，该帧无 choices → 返回 `{}` → 被忽略。流中错误的用户感知=内容戛然而止，无错误标记（非流式错误路径 :147-157 有 !res.ok 兜底，正常）。演示页面窄，P3。默认解：deltaOf 检 `error` 键映射为 msg.error。

### 21-03 eventData 单行假设（P3，健壮观察）

chat.tsx:26-29 `event.split("\n").find(l => l.startsWith("data: "))` 只取首个 data 行；SSE 规范允许多 data 行拼块。网关（axum sse）当前单行输出故不可达。默认解：保持现状+注释，或 join 全部 data 行。

### 21-04 测试缺口族（P3）

已有：login-page 5 例（init/login 表单切换、init 成功回跳、登录失败提示、已登录回跳）、chat-page 6 例（流式思考折叠/reasoning_details 回传/停止保留/清空/模型浮窗分组/错误内联）、require-auth 组件级测试（components/__tests__/require-auth.test.tsx）。缺口：21-01 的保存时序（saveInitSettings 在 onSuccess 之后）无断言；use-auth 的 login/init/logout setQueryData 与 invalidation 零直测；chat 的流内 error 帧（21-02）与畸形 JSON chunk 分支（deltaOf throw→error 标记）未测；use-init-settings 的 saveInitSettings 失败聚合（部分成功）零直测。

## 已核验无问题区（避免后续票重复审查）

- **RequireAuth 守卫**：isLoading 占位→isError/!data 跳 /login 带 from（pathname）→ 有测试；与全局 401 钩子（19-14）双通道一致。
- **useAuthAction 成功后 setQueryData 双写**（me+status）避免多余重取；useAuthStatus staleTime=Infinity+retry=false 合理。
- **useLogout**：onSettled 清 me → 守卫跳登录，失败也收敛到登录页（可接受兜底）。
- **chat 主链路**：modelKey 拆分类型正确（ProviderModel.modelId=数字 PK，与后端 chat_body 契约一致）；isComposing 守卫中文输入法；abort→stopped 标记+已收内容保留（测试在）；reasoning_details 随历史原样回传（测试在）；messages 无界增长=演示页可接受；Popover/MidEllipsis 用法合规。
- **chat 不经 ky 的 401 面**：fetch 直调无 401 全局跳转，会话过期时错误内联展示——演示页可接受，记为已知形态（01 盘点已确认与 SSE hook 零耦合合理）。
- **use-init-settings**：browserTimezone/timezoneOptions（Intl.supportedValuesOf + shortOffset 标签，未知时区回退 IANA 名）正确；types/intl.d.ts 补丁被实际使用。
- **初始化表单其余面**：init/login 双形态切换正确（status.initialized）；已登录访问 /login 回跳；language Select disabled 展示当前值（顶部 LocaleToggle 负责改）。
- **改密流程**：21 票不重复（18 票已核验：后端保留当前会话、前端不跳转=正确）。

## 性能/内存轮结论

无性能项。chat 流式为单连接逐事件 setState（patchLast 浅拷贝尾元素），消息量演示级；timezoneOptions 全量 IANA 列表一次性 useMemo；login 页无重查询。结论：本域无性能负债。
