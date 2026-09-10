# 17 · FE 配置管理域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对前端配置管理域做全量审查：`pages/providers.tsx` / `provider-models.tsx` / `virtual-models.tsx` / `provider-overview.tsx` / `virtual-model-overview.tsx` / `api-keys.tsx` + `components/providers/` / `provider-models/`（AddProviderModelsDialog 824 行等）/ `virtual-models/`（VirtualModelEditDialog 685 行等）及 hooks 与 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：编辑弹窗表单校验（zod/受控态）、镜像/继承字段门控模板、级联刷新时机、列表缓存失效（query invalidation）面；
- 实现简洁：三个大弹窗的重复表单项/校验模式、detail 弹窗与页面跳转双形态并存面；
- 测试覆盖：组件级用例之外缺什么（弹窗提交流程、字段门控矩阵）；
- 模块间调用：与共享 data-table/ui 层的契约、与后端 CRUD 字段名同步面。

范围注记（01 盘点）：本票域内含 CRUD hooks（use-providers/use-provider-models/use-virtual-models/use-api-keys/use-usage-estimate 等）；hooks 为跨域共享接口（16 域 RequestLogsTable 亦消费 4 个实体 hook），视为冻结。

产出 `.scratch/code-quality-map-2026-09-09/findings/17-fe-configuration-management.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/17-fe-configuration-management.md`——28 条（6 P2 + 22 P3）+ 弹窗脚手架收敛定案。双子代理分族全读（providers+api-keys / provider-models+virtual-models），全部 P2 与关键 P3 经主代理磁盘复核。

**六张 P2**：17-01 ProviderDetail 切换供应商时在途明文 Key 请求迟到串号（A 的密钥写到 B 的详情）；17-02 用量开关一去不返（可见性绑 `usage===true` 而非「存在 usage 键」，关一次永久消失）；17-03 useMatchTemplate 的 404 吞掉逻辑失效（await 在 try 外，ky 抛错——未命中是常态，每键击 2 请求+无防抖）；17-04 toast 用了不存在的 i18n key（`apiKeys.showKeyFailed` 两 locale 均无，显示字面量）；17-05 ProviderDetail 两处无保护 JSON.parse（密钥丢失时后端透传密文→渲染抛错整页 ErrorBoundary）；17-06 Add 弹窗 manual/pending 候选卡整卡可点，卡内数字输入无 stopPropagation——点击/回车冒泡跳转让输入框卸载，**这两个态的数字字段根本填不进去**（测试全用 fireEvent.change 是无回归网的根因）。

**弹窗脚手架上架定案（01 盘点候选）**：只抽「固定头/尾+可滚动主体」布局原语 DialogScrollShell（ProviderEditDialog 与 SpeedTestDialog 头栏 className 逐字节相同可证），不抽 form/zod/toast 装配（ADR-0004 已否决的泥潭，采纳需加 addendum 划边界）——19 票落地、17 域五弹窗消费；17-19 两个小详情弹窗的滚动形态违规正好是首批修正对象。

**P3 要点**：代理 @ 校验前端口径缺/truncate 违规一处（17-08）/ApiKeyCell 明文进 React Query 缓存与 ProviderDetail 策略相悖（17-10，波及面已标）/删除选中供应商后空白不回落/测速弹窗挂载即全局拉模型（冻结 hook 协商面已标）/useDeleteProvider 漏失效 providerModelKeys（17-20，波及 16 域）/VM 弹窗不可解析草稿成员仍提交（后端 400 前端无提示）/详情弹窗 refetch 丢编辑态（依赖对象身份）/假防抖 modelSearchDebounced/三处近重复（能力图标×2、只读 dl×2、手动表单×2）/测试缺口族（17-06 回归网缺失根因+四条 onError+接口类型确认链路零覆盖+hooks 零直测）。

**已核验无问题区**（10 项）：删除确认三弹窗、invalidation 其余面、已导入排除在后端、接口类型门控前后端一致、隐藏凭据保全、字段同步面（注意 11-27 若补 disabledReason 需手工同步前端类型）、条件 return null 在 hooks 后、custom_header 前端更严只少放行。

**需拍板问题**：无。

Status: resolved
