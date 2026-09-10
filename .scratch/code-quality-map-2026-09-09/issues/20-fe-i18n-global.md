# 20 · FE 全局文案与国际化域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对前端全局文案与国际化域做全量审查：`i18n/locales/zh-CN.ts`（781 行）/ `en.ts`（791 行）、i18n 工具层与全站文案调用点。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：中英键集合一致性（缺键/多余键）、占位符（`{}`）参数个数与顺序匹配、硬编码中文绕过 i18n 的点；
- 实现简洁：双文件结构重复、分组面是否与页面域对齐；
- 测试覆盖：有无 i18n 一致性测试（缺什么）；
- 模块间调用：i18n 键被多页消费的命名契约（新增页面如何登记）。

产出 `.scratch/code-quality-map-2026-09-09/findings/20-fe-i18n-global.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/20-fe-i18n-global.md`——10 条（2 P2 + 8 P3）无拍板。键集合主代理脚本实证（tsx 直跑）：**zh/en 各 703 键，零缺键、零多余键、零占位符不匹配**；全站被引用未定义的键仅 `apiKeys.showKeyFailed`（17-04）。硬编码中文全量扫描（剥注释 81 行逐点确认）：剔除 17/18/19 移交锚点后新实例仅 constants DEFAULT_GROUP + race-period 手写格式 9 处 + dashboard-charts 3 处 + utils 计数单位（低风险观察）。

**两张 P2**：20-01 `<html lang>` 首帧不跟随（languageChanged 处理器注册晚于 init 的同步触发，i18next 26.4.0 实证漏首帧；index.html 硬编码 zh-CN，英文用户首屏 lang 恒错到手动切换；修复=init 后显式同步一次，一行）；20-02 zh-CN 的 dashboard.success/failed 写的是英文值（"success"/"failed"）→ 中文界面 insight tooltip 显示英文、与同图硬编码中文图例（16-05）同图矛盾（修复=zh 值改中文+图例改走该批键，两票合流）。

**P3 要点**：死键 160/703（usage 13/13、time 5/5 整组全死，7 页面 `<域>.title` 与 nav.pages 重复）/手写日期格式绕 i18n 而 time.* 全组死键（两套文案来源并存）/DEFAULT_GROUP 硬编码「默认」/全局 i18n.t 非响应式直调靠父组件兜底（易碎模式）/**无任何 i18n 制度化校验**（本票键集合核验是手工脚本，一个一致性用例即可制度化抓 17-04 类）/注释漂移×2/i18n 初始化靠隐式副作用（main.tsx 无显式 import）/language 设置前端只写不读（18-03 同族补面）。

**需拍板问题**：无。

Status: resolved
