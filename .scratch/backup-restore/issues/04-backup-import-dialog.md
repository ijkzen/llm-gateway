# 04: 前端导入弹窗（拖拽/选择/确认/反馈）

**What to build:** 导入弹窗：上部拖拽区（原生 drag 事件，拖入高亮+显示文件名）+ 下部手动选择文件 + 底部「确认导入」按钮（未选文件禁用）。点确认先弹破坏性确认（明确列出将替换的配置类型 + 明文密钥警告）；确认后读文件文本调 `POST /api/backup/import`。成功 Toast「成功导入」并刷新各配置列表；失败（400/网络错）弹具体错误弹窗展示后端错误消息。文案走 i18n。

**Blocked by:** 02（依赖后端导入端点）、03（由备份弹窗的恢复按钮打开）

**Status:** ready-for-agent

- [ ] 导入弹窗含拖拽区 + 「手动选择文件」（隐藏 file input，accept .json）+ 「确认导入」
- [ ] 拖入文件 / 选择文件后显示文件名；确认按钮可用性随文件有无切换
- [ ] 点确认先弹破坏性确认（AlertDialog）：文案明确「将清空并替换全部供应商/虚拟模型/API Key/系统设置」+ 明文密钥警告
- [ ] 确认后提交：成功 Toast「成功导入」+ 关闭 + 刷新 providers/provider-models/virtual-models/api-keys/settings 查询
- [ ] 失败弹错误弹窗：展示后端 400 具体消息；网络错误给通用提示
- [ ] i18n key（zh/en）就位
- [ ] 组件测试：拖拽/选择显示文件名、确认流程→成功 toast、失败→错误弹窗

**Blocked by:** 02-backup-import, 03-backup-dialog-export
