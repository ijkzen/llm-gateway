# FINDINGS · 20 FE 全局文案与国际化域审查（2026-09-10）

范围：`i18n/index.ts` 工具层全读 + `locales/zh-CN.ts`（781 行）/`en.ts`（791 行）结构审读 + 全站硬编码中文扫描（剥离注释后 tsx/ts 残留 CJK 81 行逐点确认）+ i18n 测试面盘点。方法：**主代理脚本实证键集合**（tsx 直跑）+ 一个子代理审工具层与绕过点（i18next 26.4.0 node_modules 实证事件时序），P2 经主代理磁盘复核。清单模式：不改代码。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 20-01 | P2【已修复 2026-09-10】 | 逻辑 | `<html lang>` 首帧不跟随语言：`languageChanged` 处理器注册在 `i18n.init()` 之后，而该事件在 init 内同步触发（i18next 26.4.0 实证）→ 处理器漏掉首帧；index.html 硬编码 `lang="zh-CN"`，英文用户首屏 lang 恒 zh-CN 直到手动切换 |
| 20-02 | P2【已修复 2026-09-10】 | 逻辑 | `zh-CN.ts:681-682` 的 `dashboard.success`/`dashboard.failed` 中文档写英文值（"success"/"failed"）→ insight 失败趋势 tooltip 中文界面显示英文，且与同图硬编码中文图例（16-05）同图矛盾 |
| 20-03 | P3【已修复 2026-09-10】 | 简洁 | 703 键中 160 键零引用（死键）：`usage`（13/13）与 `time`（5/5）整组全死、`error.internalError/unauthorized`、`providers.usageLabels.*`（14）、7 页面的 `<域>.title` 与 `nav.pages.*.title` 语义重复 |
| 20-04 | P3【已修复 2026-09-10】 | 逻辑/简洁 | 日期/周期格式化手写「locale==="zh" ? 中文串 : Intl」（race-period.ts 9 处 + dashboard-charts.tsx 3 处）绕过 i18n，而所需格式的 `time.*` 键全组死键——两套文案来源并存 |
| 20-05 | P3【已修复 2026-09-10】 | 逻辑 | `constants.ts:1` `DEFAULT_GROUP = "默认"` 硬编码，英文界面 cron 列表/详情显示中文「默认」 |
| 20-06 | P3【已修复 2026-09-10】 | 模块间 | insight-charts.tsx/ProtocolIcon.tsx 直调全局 `i18n.t`/`i18n.language` 不订阅 useTranslation——语言切换靠父组件恰好重渲染兜底，被 memo 或独立复用即成陈旧文案（当前无可复现，易碎模式） |
| 20-07 | P3【已修复 2026-09-10】 | 测试覆盖 | 无任何 i18n 制度化校验（键集合/占位符/引用键存在性/硬编码守卫全缺）：本票的「703 键零漂移」是主代理手工脚本验证，仓内无测试固化；一个一致性用例即可制度化抓住 17-04 类缺键 |
| 20-08 | P3【已修复 2026-09-10】 | 注释漂移 | en.ts:3 注释称 satisfies 实为 `: Translation`；index.ts:42 注释检测链尾「→ zh-CN」与实现（非中文 → en，zh-CN 仅 fallbackLng）不符 |
| 20-09 | P3【已修复 2026-09-10】 | 模块间 | i18n 初始化靠隐式副作用：main.tsx 未显式 import `@/i18n`，仅经 App→use-locale 链触发；入口移除该 hook 即静默丢失初始化（test/setup.ts 显式 import 说明作者知情） |
| 20-10 | P3【已修复 2026-09-10】 | 模块间 | `language` 设置前端只写不读（PUT 三处、零 GET）——18-03 同族补面：设置页直改语言后端已同步 cron 标题而前端仍旧语 |

**本票无需拍板项**（16-05 图例修复与 20-02 同键合流；死键清理归图后实施批）。

## 键集合核验（主代理脚本实证）

tsx 直跑比对 zh-CN.ts/en.ts：**各 703 键，零缺键、零多余键、零占位符不匹配**。另有编译期双保险：`en.ts` 以 `: Translation` 标注强制形状一致。全站「被引用但未定义」的键仅 `apiKeys.showKeyFailed` 一个（17-04 已记；i18next 缺键返回原始 key 字符串，无 returnNull 配置）。无动态 key 拼接（`t(\`…\`)` 模板调用零命中），死键判定可靠。

## 硬编码中文全量清单（剔除 17/18/19 票已知锚点后的新实例）

| 位置 | 内容 | 归条 |
| --- | --- | --- |
| lib/constants.ts:1 | `DEFAULT_GROUP = "默认"` | 20-05 |
| lib/race-period.ts:260/268/272/276/279/286/292/297/301 | 「（当前）」「年/月/日/第N周」手写格式 | 20-04 |
| components/dashboard-charts.tsx:84/88/91 | 「月/日/年」手写格式 | 20-04 |
| lib/utils.ts:83/86/101/104/107/110 | 亿/万计数单位（仅 zh 分支出现，en 分支不可达） | 观察（低风险 locale 数据） |

已知锚点（移交汇总，不重复开条）：settings 域五文件（18 票末节锚点清单）、ProviderUsageCard.tsx:55/64/202（17-16）、ProviderModelSection.tsx:69-72（17-21）、error-boundary.tsx:29-36（19-10）、insight-charts 图例（16-05）、dashboard-charts OTHER_LABEL（16-12）、utils.ts formatDateTime（19-20）。**全站扫描确认：除此之外无其他注释外中文字面量。**

## 各条证据（P2 两条）

### 20-01 `<html lang>` 首帧不跟随（P2）

i18n/index.ts:46-62：`void i18n.use(...).init({...})`（:46-56）之后才 `i18n.on("languageChanged", ...)`（:60-62）。i18next 26.4.0 实证：init 内同步触发一次 languageChanged，后注册的处理器收 0 次 → 首帧 html lang 不更新；index.html:2 硬编码 `lang="zh-CN"`；useInitLocale（use-locale.ts:43-48）只在 i18n.language !== locale 时补触发而首帧二者恒等 → 英文用户页面 lang 恒 zh-CN 直到手动切换。影响：浏览器原生 UI（日期选择器等）语言与页面语言不一致，无障碍/语义面。默认解：init 后立即显式执行一次 `document.documentElement.lang = i18n.language`（一行）。

### 20-02 中文档英文值（P2）

zh-CN.ts:681-682 `success: "success", failed: "failed"`（dashboard 分组，同组其余键如 failureRate:"失败率" 均中文）；insight-charts.tsx:210-213 tooltip 用 `i18n.t("dashboard.success")/("dashboard.failed")` → 中文界面 tooltip 显示英文；同图图例硬编码「成功/失败」（16-05）→ 同图双语自相矛盾。默认解：zh 值改「成功/失败」+ 图例改走该批键（16-05 合流修复）。

## 已核验无问题区（避免后续票重复审查）

- **初始化/检测/回退自洽**：storedLocale（zustand persist 键两端一致）→ detectBrowserLocale（zh→zh-CN 其余 en）→ fallbackLng zh-CN；资源内联 init 同步完成（isInitialized 实测真），useSuspense 不挂起；interpolation.escapeValue=false 对 React 正确。
- **缺键行为**：返回原始 key 字符串（17-04 的机制确认）。
- **复数形态**：relativeTime.*Ago 无复数变体但 {count} 插值实测回退正确。
- **全局实例同源**：lib/api.ts、ProtocolIcon、insight-charts 的全局 i18n 与 useTranslation 同一默认实例。
- **测试环境初始化**：setup.ts 先写 zh-CN localStorage 再动态 import，规避静态提升读 jsdom en-US——正确的隐式依赖管理（对照 20-09 的生产侧缺口）。
- **zh-CN.ts:545-564 settings.\* 键组与 en.ts 对应段齐备**（18 票佐证：settings 域组件未使用这些既有键）。

## 测试覆盖盘点

无任何 i18n 专项测试（55 文件 417 用例中零）：键集合/占位符一致性、「引用键必须存在」、硬编码中文守卫、locale-toggle/use-locale 全缺；CI 无 i18n 步骤。20-07 给制度化方案（一个 locales 一致性用例+一个引用键存在性用例，可直接抓 17-04/20-02 类）。页面测试仅 zh-CN 断言，en 侧文案无回归网。

## 性能/内存轮结论

无性能负债。唯一观察：useChangeLocale 切换成功即 `invalidateQueries()` 全量失效（含统计/赛马重查询）——为刷新后端本地化 cron 标题有意为之，可用更窄 key 前缀收窄（图后可选）。

## 实施进度（2026-09-10）

- **20-03 已修复**：脚本实证后删除了 104 个零引用死键（zh-CN 与 en 各删 110 行，两份文件对称），清理后键数 711 → 607、复查死键为 0——此前列出的 usage/time 整组全死已随本次清理消失（time.* 由 20-04 复用后重新加回）。
- **20-04 已修复**：`race-period.ts` 的 `formatPeriodLabel`（含 tz 与非 tz 两条路径）与 `dashboard-charts.tsx` 的 `formatBucketLabel` 的中文分支改走 `time.*` 词条（新增 `time.currentSuffix`），英文侧保持既有 Intl 格式不变（既有断言全绿）。
- **20-05 已修复**：新增 `cronJobs.defaultGroup` 双语词条；`CronJobList`/`CronJobDetail` 展示时按翻译渲染，`DEFAULT_GROUP` 常量保留为后端种子值比较用（注释说明）。
- **20-06 已修复**：`insight-charts` 五个图表组件与 `ProtocolIcon` 改用 `useTranslation()` 订阅语言变化（纯函数 `protocolLabel`/`billingModeLabel` 保持全局实例读，注释标注调用方需保证重渲染）。
- **20-07 已修复**：新增 `i18n/__tests__/locales-consistency.test.ts`——中英键集合一致、同键占位符一致、源码引用的 `t("域.key")` 必须存在（经 `import.meta.glob` 读源码 + 新增 `src/vite-env.d.ts`）。该用例当场抓出一处真实缺键：`lib/api.ts` 引用的 `error.requestAborted` 实际键为 `error.aborted`（已修）。
- **20-08 已修复**：`en.ts` 注释由「用 satisfies」改为「以 `: Translation` 标注」；`index.ts` 的初始语言注释改为与实现一致（localStorage → 浏览器语言检测，zh-CN 仅作 fallbackLng）。
- **20-09 已修复**：`main.tsx` 显式 `import "@/i18n"`，不再依赖 App→use-locale 的隐式副作用链。
- **20-10 已修复**：新增 `useSyncBackendLocale`（在 AppLayout 已认证场景调用），拿到设置表后把本地 store 与 i18n 对齐到后端 `language`——多客户端共用同一网关时前端不再与后端分叉。

- **20-01 已修复**：`i18n/index.ts` 的 `languageChanged` 处理器注册移到 `init()` 之前，且 init 后直接 `document.documentElement.lang = initial`——首帧 `<html lang>` 跟随实际语言（原先英文用户首屏恒 zh-CN）。
- **20-02 已修复**：`zh-CN` 的 `dashboard.success/failed` 值由 "success"/"failed" 改为「成功」/「失败」；insight 五图图例改走 i18n（16-05），同图 tooltip 与图例语言一致。
