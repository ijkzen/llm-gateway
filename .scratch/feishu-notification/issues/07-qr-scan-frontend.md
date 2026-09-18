# 07: 前端 —— 扫码创建路径

**What to build:** 在通知弹窗里点「扫码创建」弹出二维码，管理员用飞书客户端扫码确认后，表单被自动填好（App ID / App Secret / 接收者 = 创建者 open_id），确认后保存。

**Blocked by:** 03（注册会话接口）、06（弹窗骨架与 i18n）

**Status:** ready-for-agent

- [ ] 扫码区：点「扫码创建」→ `POST register` → 用 `QRCodeSVG` 渲染二维码（内容为后端返回的 `qrUrl`）
- [ ] 轮询状态（TanStack Query `refetchInterval`，拿到终态即停）：等待扫码 / 已扫码待确认 / 成功 / 已过期，文案走 i18n
- [ ] 成功后**回填表单**（appId / appSecret / receiver，receiverType = `open_id`）并切到编辑态 + toast 提示；**不自动保存**——由用户确认后再点保存
- [ ] 过期或 `denied` 后提供「重新获取」二维码
- [ ] i18n：扫码相关文案补 `zh-CN.ts` 与 `en.ts`
- [ ] 组件测试（fake timers 驱动 `refetchInterval`）：点扫码 → 断言渲染二维码 → 轮询 `pending` → `success` → 表单被回填；过期态显示「重新获取」
