# 06: 前端 —— 设置页入口 + 通知弹窗（手动创建 / 修改 / 测试发送）

**What to build:** 设置页备份按钮**左侧**多一个「消息通知」按钮，点开弹窗可以手动填 App ID / App Secret / 接收者创建机器人、查看并修改已配置的机器人、点按钮发一条测试消息。

**Blocked by:** 01, 02（需要配置读写与测试发送接口）

**Status:** ready-for-agent

- [ ] 新增依赖 `qrcode.react@4.2.0`（peer 支持 React 19）——07 会用到，本工单一并装上
- [ ] `web/src/lib/notification.ts` API 模块（照 `lib/backup.ts` 形态）+ hooks（配置查询 / 保存 / 测试发送）
- [ ] 设置页 `PageHeader` 操作区内、备份按钮**左侧**新增按钮（`Bell` 图标 + 「消息通知」）
- [ ] `NotificationDialog.tsx`：
  - 未配置态展示**两条创建路径**（本工单实现「手动填写」：App ID / App Secret / 接收者类型 / 接收者；「扫码创建」入口先留位，由 07 补齐）
  - 已配置态展示 appId、掩码 appSecret、接收者、启用开关、最近一次发送结果（`lastError` / `lastSentAt`）；可进入编辑态，**Secret 输入框留空 = 不改**（占位文案说明）
  - 停用/启用开关直接生效
- [ ] 测试发送按钮（底部操作区左侧）：用**表单当前值**调用 test 接口（编辑态未保存也能测）；成功 toast、失败 toast 展示后端返回的中文原因
- [ ] 弹窗遵守仓库规范：标题栏与底部操作区**固定**、仅中间内容区滚动（照 `ProviderSpeedTestDialog`）
- [ ] i18n：`zh-CN.ts` 与 `en.ts` 新增 `notification` 命名空间
- [ ] 组件测试（`web/src/components/__tests__/notification-dialog.test.tsx`，mock API 模块，沿用现有 dialog 测试的 `act` + mock 模式）：未配置态两条路径可见；手动保存 → 断言 PUT 请求体；已配置态掩码展示；Secret 留空提交 → 请求体**不带** `appSecret`；测试发送成功 / 失败展示
- [ ] 设置页回归测试（扩展 `web/src/__tests__/settings-page.test.tsx`）：断言「消息通知」按钮存在且位于备份按钮**左侧**（DOM 顺序）
