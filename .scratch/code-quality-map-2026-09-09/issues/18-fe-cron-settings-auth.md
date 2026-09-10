# 18 · FE 任务/设置域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对前端任务/设置域做全量审查：`pages/cron-jobs.tsx` / `settings.tsx` / `not-found.tsx` + `components/cron-jobs/`（CronJobLogsDialog 等）+ `components/settings/` 组件族（SettingEditDialog/JsonSettingEditDialog/SettingsTable/ChangePasswordDialog/BackupDialog 等）+ use-cron-jobs/use-cron-job-logs/use-settings 及 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：SSE 日志流的快照/增量/重置状态机（MockEventSource 已锁行为不复查）、设置类型表单与后端声明类型一致性、弹窗滚动区约定（已收口不复查）；
- 实现简洁：表单构造重复、SSE hook 状态机复杂度；
- 测试覆盖：settings 域组件直测缺口（SettingEditDialog 无专属测试、settings.tsx 页无测试——01 盘点已标薄弱点）、CronJobLogsDialog 之外缺什么；
- 模块间调用：use-cron-job-logs 与后端 SSE 协议契约、settings 组件 i18n 绕过（49 处硬编码中文，归 20 票文案改造——本票只记录不重复）。

范围注记（01 盘点）：login/auth/chat 已拆至 21 票；hooks 为跨域共享接口，视为冻结。

产出 `.scratch/code-quality-map-2026-09-09/findings/18-fe-cron-settings-auth.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/18-fe-cron-settings-auth.md`——20 条（4 P2 + 16 P3）无拍板。双子代理分域全读（cron-jobs / settings），全部 P2 与关键 P3 经主代理磁盘复核。

**四张 P2**：18-01 **08-01 前端半边实证**——后端实时 log 事件从不带 seq（写死 None+skip 序列化），前端 `data.seq<=last.seq` 去重因 undefined 比较恒 false 而**失效**（快照重叠必现重复行）+ 实时日志 React key 恒 undefined；测试 emitLog 注入 seq 构成假阳性契约（08-01 拍板落地后自愈，过渡期前端 seq 改可选+key 换稳定回退）。18-02 reset 分支 fetchQuery 整体替换 logs 的竞态（替换丢增量+staleTime 5min 可能命中旧缓存回退更多）。18-03 设置表直改 language 不热切换前端 i18n（正路 useChangeLocale 只有 locale-toggle 在用；前后端语言长期分叉无提示，改法在调用侧不动冻结 hook）。18-04 SettingEditDialog 类型盲（非 Json 一律纯文本 z.string()，Int/Bool/Float 无控件无校验，非法值提交后才吃 400）。

**P3 要点**：SSE 重连 idle 不 invalidate runs/立即执行 1s 刷新拿不到 last_run_at（结束才回写）/删除弹窗 onError 关弹窗与四处同类不一致/实时日志整表重渲染 O(n²)/SSE onerror 恒 reconnecting 无终止（401 永久重连中）/Json 结构化编辑静默丢数据（测试反而固化）/ImportDialog 文件状态残留可重复误导入/改密长度 JS 字符 vs 后端字节/SettingEditDialog 不展示 key/SettingsTable pageIndex effect 与框架默认重叠冗余/测试缺口两族（cron 域 reset/run_started 等分支全裸+四组件零测试；settings 页与 SettingEditDialog 无测试=01 盘点两项确认）。

**i18n 锚点汇总已归集给 20 票**（settings 域五文件 + 17 票两实例；佐证=zh-CN.ts:545-564 与 en.ts 已有 settings.* 键未被使用，20 票可直接复用）。

**已核验无问题区**：SSE 契约其余面（事件名/归属过滤/快照先行/卸载关闭）、弹窗固定头底本域全合规、CRUD 字段同步、改密流程（保留当前会话=正确）、备份链路错误展示、时区变更前端即时性。

**需拍板问题**：无。

Status: resolved
